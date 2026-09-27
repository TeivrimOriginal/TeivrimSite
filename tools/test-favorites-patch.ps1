# Verifies the PATCH semantics of POST /api/favorites against a running server.
#
#   powershell -File tools\test-favorites-patch.ps1 -BaseUrl http://127.0.0.1:8100
#
# The contract has three states per field and all three have to be reachable:
#
#   field absent          -> leave the stored value alone
#   field = null          -> clear it
#   field = a value       -> set it
#
# The middle one is the reason this exists: serde collapses `Option<Option<T>>`
# for "absent" and "null" into the same `None`, so a client could set a score but
# never remove it.

[CmdletBinding()]
param(
    [string]$BaseUrl = "http://127.0.0.1:8100"
)

$ErrorActionPreference = "Stop"
$script:failures = 0

function Check($name, $expected, $actual) {
    if ($expected -eq $actual) {
        Write-Host "PASS  $name" -ForegroundColor Green
    } else {
        Write-Host "FAIL  $name`n        expected: $expected`n        actual:   $actual" -ForegroundColor Red
        $script:failures++
    }
}

function Post-Json($path, $body, $token) {
    $headers = @{ "Content-Type" = "application/json" }
    if ($token) { $headers["Authorization"] = "Bearer $token" }
    $json = if ($null -eq $body) { "{}" } else { $body | ConvertTo-Json -Compress }
    $response = Invoke-WebRequest -Uri "$BaseUrl$path" -Method Post -Body $json -Headers $headers `
        -UseBasicParsing -TimeoutSec 30
    return ($response.Content | ConvertFrom-Json)
}

# ------------------------------------------------------------------ setup

$stamp = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
$username = "patch$stamp"
$password = "patch-test-password"

$auth = Post-Json "/api/auth/register" @{ username = $username; password = $password } $null
$token = $auth.token
Write-Host "registered $username"

$list = Invoke-RestMethod -Uri "$BaseUrl/api/anime?per_page=1" -TimeoutSec 30
$uid = $list.items[0].uid
Write-Host "using $uid"
Write-Host ""

# ------------------------------------------------------------------ tests

# A first write, so there is something to preserve afterwards.
$entry = Post-Json "/api/favorites" @{ uid = $uid; status = "watching"; score = 8; progress = 5; notes = "hello" } $token
Check "initial write stores the score" 8 $entry.score
Check "initial write stores the progress" 5 $entry.progress
Check "initial write stores the notes" "hello" $entry.notes

# Absent: leave alone.
$entry = Post-Json "/api/favorites" @{ uid = $uid; is_favorite = $true } $token
Check "absent score is left alone" 8 $entry.score
Check "absent progress is left alone" 5 $entry.progress
Check "absent notes are left alone" "hello" $entry.notes
Check "absent status is left alone" "watching" $entry.status
Check "is_favorite is applied" $true $entry.is_favorite

# Explicit null: clear.
$entry = Post-Json "/api/favorites" @{ uid = $uid; score = $null } $token
Check "null score is cleared" $null $entry.score
Check "clearing the score keeps the progress" 5 $entry.progress
Check "clearing the score keeps the notes" "hello" $entry.notes

$entry = Post-Json "/api/favorites" @{ uid = $uid; progress = $null; notes = $null } $token
Check "null progress is cleared" $null $entry.progress
Check "null notes are cleared" $null $entry.notes
Check "cleared fields leave the star alone" $true $entry.is_favorite
Check "cleared fields leave the status alone" "watching" $entry.status

# Set again, then clear through the explicit-null route the client uses.
$entry = Post-Json "/api/favorites" @{ uid = $uid; score = 6; progress = 2; notes = "again" } $token
Check "score can be set again" 6 $entry.score
Check "progress can be set again" 2 $entry.progress
Check "notes can be set again" "again" $entry.notes

# The status stays validated whatever happens to the others.
$rejected = $false
try {
    Post-Json "/api/favorites" @{ uid = $uid; status = "nonsense" } $token | Out-Null
} catch {
    $rejected = $true
}
Check "an unknown status is rejected" $true $rejected

$rejected = $false
try {
    Post-Json "/api/favorites" @{ uid = $uid; score = 42 } $token | Out-Null
} catch {
    $rejected = $true
}
Check "an out-of-range score is rejected" $true $rejected

# And the read path agrees with what was written.
$rows = Invoke-RestMethod -Uri "$BaseUrl/api/favorites?limit=10" -Headers @{ Authorization = "Bearer $token" } -TimeoutSec 30
$row = @($rows) | Where-Object { $_.uid -eq $uid } | Select-Object -First 1
Check "the list endpoint agrees on the score" 6 $row.library.score
Check "the list endpoint agrees on the status" "watching" $row.library.status
Check "the list endpoint agrees on the star" $true $row.library.is_favorite

# ---------------------------------------------------------------- teardown

Invoke-WebRequest -Uri "$BaseUrl/api/favorites/$([uri]::EscapeDataString($uid))" `
    -Method Delete -Headers @{ Authorization = "Bearer $token" } -UseBasicParsing -TimeoutSec 30 | Out-Null
$after = Invoke-RestMethod -Uri "$BaseUrl/api/favorites?limit=10" -Headers @{ Authorization = "Bearer $token" } -TimeoutSec 30
Check "the entry is gone after delete" 0 (@($after) | Where-Object { $_.uid -eq $uid }).Count

Write-Host ""
if ($script:failures -gt 0) {
    Write-Host "$script:failures check(s) failed" -ForegroundColor Red
    exit 1
}
Write-Host "All PATCH-semantics checks passed." -ForegroundColor Green
