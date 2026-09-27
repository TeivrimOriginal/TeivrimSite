# Publishes a build to RuStore through the RuStore Connect HTTP API.
#
#   powershell -File tools\rustore-upload.ps1 -FilePath android\app\build\outputs\bundle\release\app-release.aab
#   powershell -File tools\rustore-upload.ps1 -FilePath app-release.aab -WhatIf
#   powershell -File tools\rustore-upload.ps1 -FilePath app-release.aab -DraftStrategy delete_and_create -Commit
#
# The API is a five-step conversation, not one multipart POST:
#
#   1. POST /public/auth                                   -> Public-Token
#   2. GET  /public/v1/application/{pkg}/version?...DRAFT   -> existing draft, if any
#   3. POST /public/v1/application/{pkg}/version            -> create the draft
#   4. POST /public/v1/application/{pkg}/version/{id}/aab   -> upload the bundle
#   5. POST /public/v1/application/{pkg}/version/{id}/commit
#
# Step 1 is why the older "access_token + signature in the query string" scheme
# is not used here: that one is retired. The current token is a JWE derived from
# an RSA-SHA512 signature over `keyId + timestamp`.
#
# Runs on Windows PowerShell 5.1 as well as PowerShell 7: the RSA key is parsed
# from DER here and the multipart body is assembled by hand, because
# `ImportRSAPrivateKey` and `Invoke-WebRequest -Form` are 7-only.
#
# Credentials
# -----------
# Both values come from the developer console (RuStore -> API keys) and are
# session secrets. Neither belongs in the repository or on a command line where
# the shell history keeps it.
#
#   $env:RUSTORE_KEY_ID      = "...."
#   $env:RUSTORE_PRIVATE_KEY = "...."
#
# RUSTORE_PRIVATE_KEY is the body of the PEM without the BEGIN/END lines. Both
# PKCS#1 ("BEGIN RSA PRIVATE KEY") and PKCS#8 ("BEGIN PRIVATE KEY") are accepted.

[CmdletBinding(SupportsShouldProcess)]
param(
    [Parameter(Mandatory = $true)]
    [string]$FilePath,

    [string]$PackageName = "ru.teivrim.anime",

    # Store metadata. Kept in a file rather than in parameters because it is
    # long-form Russian prose that has to be reviewed, not typed. Empty means
    # "next to this script"; $PSScriptRoot is not bound yet in a param default
    # when the file is run with -File, so the real path is resolved below.
    [string]$MetadataPath = "",

    # What to do when a draft already exists for this package.
    [ValidateSet("reuse_existing", "delete_and_create", "fail_if_exists")]
    [string]$DraftStrategy = "reuse_existing",

    # Uploading a draft and submitting it for review are separate steps on
    # purpose: a first upload usually needs the listing fixed in the console
    # before it is worth spending a moderation slot on.
    [switch]$Commit,

    # Use an existing draft id directly and skip both lookup and creation.
    [string]$VersionId = ""
)

$ErrorActionPreference = "Stop"
$Base = "https://public-api.rustore.ru"

function Fail($message) {
    Write-Host ""
    Write-Host "FAILED: $message" -ForegroundColor Red
    Write-Host ""
    exit 1
}

function Step($text) { Write-Host "-> $text" -ForegroundColor Cyan }

function Ok($text) { Write-Host "   $text" -ForegroundColor Green }

# ------------------------------------------------------------------ checks

$resolved = Resolve-Path $FilePath -ErrorAction SilentlyContinue
if (-not $resolved) { Fail "File not found: $FilePath" }
$bundle = $resolved.Path

if (-not $MetadataPath) {
    $here = Split-Path -Parent $MyInvocation.MyCommand.Path
    $MetadataPath = Join-Path $here "rustore-app.json"
}

$metadata = $null
if ($MetadataPath -and (Test-Path $MetadataPath)) {
    $metadata = Get-Content $MetadataPath -Raw -Encoding UTF8 | ConvertFrom-Json
} elseif (-not $VersionId) {
    Fail "No metadata file at $MetadataPath. Pass -MetadataPath, or -VersionId to reuse a draft."
}

$required = @("RUSTORE_KEY_ID", "RUSTORE_PRIVATE_KEY")
$missing = @($required | Where-Object {
    # [Environment]::GetEnvironmentVariable rather than $env:$_ : without the
    # braces PowerShell reads the underscore as part of a drive-qualified name.
    [string]::IsNullOrWhiteSpace([Environment]::GetEnvironmentVariable($_))
})

Write-Host ""
Write-Host "RuStore upload" -ForegroundColor Cyan
Write-Host "  bundle     : $bundle ($([math]::Round((Get-Item $bundle).Length / 1MB, 2)) MB)"
Write-Host "  package    : $PackageName"
Write-Host "  draft      : $DraftStrategy"
Write-Host "  commit     : $(if ($Commit) { 'yes' } else { 'no (upload only)' })"
if ($metadata) { Write-Host "  metadata   : $MetadataPath" }

$dryRun = ($missing.Count -gt 0) -or $WhatIfPreference

if ($dryRun) {
    if ($missing.Count -gt 0) {
        Write-Host ""
        Write-Host "Missing environment variables: $($missing -join ', ')" -ForegroundColor Yellow
        Write-Host "Everything that can be checked without credentials is checked below."
        Write-Host "See docs/RUSTORE.md for where to get them."
    }
    Write-Host ""
    Write-Host "DRY RUN - nothing is sent." -ForegroundColor Yellow
    Write-Host ""
    Write-Host "  1. POST $Base/public/auth"
    Write-Host "  2. GET  $Base/public/v1/application/$PackageName/version?versionStatuses=DRAFT"
    if ($VersionId) {
        Write-Host "  3. skipped (-VersionId $VersionId)"
    } elseif ($metadata) {
        Write-Host "  3. POST $Base/public/v1/application/$PackageName/version"
    }
    Write-Host "  4. POST $Base/public/v1/application/$PackageName/version/<id>/aab"
    if ($Commit) {
        Write-Host "  5. POST $Base/public/v1/application/$PackageName/version/<id>/commit"
    }
    if ($metadata) {
        Write-Host ""
        Write-Host "Metadata that would be sent:"
        foreach ($property in $metadata.PSObject.Properties) {
            $value = if ($property.Value -is [string]) { $property.Value } else { ($property.Value | ConvertTo-Json -Compress) }
            if ($value.Length -gt 88) { $value = $value.Substring(0, 88) + "..." }
            Write-Host ("  {0,-18}: {1}" -f $property.Name, $value)
        }
    }
    Write-Host ""
    exit 0
}

# ------------------------------------------------------- minimal DER reader
#
# Only what an RSA private key needs: INTEGERs inside a SEQUENCE, optionally
# wrapped in a PKCS#8 OCTET STRING. Anything richer would mean a .NET 7-only
# API, and this script has to run on the PowerShell that ships with Windows.

function Read-DerLength([byte[]]$buf, [ref]$pos) {
    $first = $buf[$pos.Value]
    $pos.Value++
    if ($first -lt 0x80) { return $first }
    $count = $first -band 0x7F
    $length = 0
    for ($i = 0; $i -lt $count; $i++) {
        $length = ($length -shl 8) -bor $buf[$pos.Value]
        $pos.Value++
    }
    return $length
}

function Read-DerInteger([byte[]]$buf, [ref]$pos) {
    if ($buf[$pos.Value] -ne 0x02) { throw "expected DER INTEGER at offset $($pos.Value)" }
    $pos.Value++
    $length = Read-DerLength $buf $pos
    $slice = New-Object byte[] $length
    [Array]::Copy($buf, $pos.Value, $slice, 0, $length)
    $pos.Value += $length
    # A leading zero is the sign byte of a positive value, not part of the
    # number. RSAParameters wants raw unsigned big-endian bytes.
    if ($length -gt 1 -and $slice[0] -eq 0) {
        $trimmed = New-Object byte[] ($length - 1)
        [Array]::Copy($slice, 1, $trimmed, 0, $length - 1)
        return ,$trimmed
    }
    return ,$slice
}

function Read-Pkcs1Numbers([byte[]]$buf, [ref]$pos) {
    [void](Read-DerInteger $buf $pos)          # version, always 0
    $n = Read-DerInteger $buf $pos
    $e = Read-DerInteger $buf $pos
    $d = Read-DerInteger $buf $pos
    $p = Read-DerInteger $buf $pos
    $q = Read-DerInteger $buf $pos
    $dp = Read-DerInteger $buf $pos
    $dq = Read-DerInteger $buf $pos
    $qinv = Read-DerInteger $buf $pos

    return [System.Security.Cryptography.RSAParameters]@{
        Modulus = $n
        Exponent = $e
        D = $d
        P = $p
        Q = $q
        DP = $dp
        DQ = $dq
        InverseQ = $qinv
    }
}

function Read-SequenceHeader([byte[]]$buf, [ref]$pos) {
    if ($buf[$pos.Value] -ne 0x30) { throw "expected DER SEQUENCE at offset $($pos.Value)" }
    $pos.Value++
    [void](Read-DerLength $buf $pos)
}

function Read-PrivateKeyParams([byte[]]$buf) {
    $pos = 0
    Read-SequenceHeader $buf ([ref]$pos)

    # Both encodings open with a version INTEGER. What follows tells them apart:
    # PKCS#8 has an AlgorithmIdentifier SEQUENCE there, PKCS#1 has the modulus.
    $numbersStart = $pos
    [void](Read-DerInteger $buf ([ref]$pos))

    if ($buf[$pos] -eq 0x30) {
        $pos++
        # Read-DerLength only consumes the length field; the AlgorithmIdentifier
        # content still has to be stepped over to reach the OCTET STRING. The two
        # statements are separate on purpose: `$pos += Read-DerLength ...` may
        # read $pos before the call advances it through the reference.
        $algorithmLength = Read-DerLength $buf ([ref]$pos)
        $pos += $algorithmLength
        if ($buf[$pos] -ne 0x04) { throw "expected PKCS#8 privateKey OCTET STRING" }
        $pos++
        $length = Read-DerLength $buf ([ref]$pos)
        $inner = New-Object byte[] $length
        [Array]::Copy($buf, $pos, $inner, 0, $length)

        # The OCTET STRING holds a whole PKCS#1 key, so it has its own SEQUENCE
        # header to step over before the numbers begin.
        $innerPos = 0
        Read-SequenceHeader $inner ([ref]$innerPos)
        return Read-Pkcs1Numbers $inner ([ref]$innerPos)
    }

    # PKCS#1 continues straight into the numbers; rewind so the number reader,
    # which skips the version itself, starts where it expects to.
    return Read-Pkcs1Numbers $buf ([ref]$numbersStart)
}

function New-RsaFromPrivateKey([string]$body) {
    $clean = ($body -replace '-----[A-Z ]+-----', '') -replace '\s', ''
    if ([string]::IsNullOrWhiteSpace($clean)) { throw "private key body is empty" }
    $der = [Convert]::FromBase64String($clean)
    $parameters = Read-PrivateKeyParams $der

    $rsa = [System.Security.Cryptography.RSA]::Create()
    try {
        $rsa.ImportParameters($parameters)
    } catch {
        $rsa.Dispose()
        throw "could not import the private key: $($_.Exception.Message)"
    }
    return $rsa
}

# ------------------------------------------------------------------- HTTP

function Invoke-Api {
    param(
        [Parameter(Mandatory = $true)][string]$Method,
        [Parameter(Mandatory = $true)][string]$Uri,
        [hashtable]$Headers = @{},
        $Body,
        [byte[]]$RawBody,
        [string]$ContentType
    )

    $args = @{
        Method         = $Method
        Uri            = $Uri
        Headers        = $Headers
        UseBasicParsing = $true
        TimeoutSec     = 600
        ErrorAction    = "Stop"
    }
    if ($null -ne $ContentType) { $args["ContentType"] = $ContentType }
    if ($null -ne $Body) { $args["Body"] = ($Body | ConvertTo-Json -Depth 8 -Compress) }
    if ($null -ne $RawBody) { $args["Body"] = $RawBody }

    try {
        $response = Invoke-WebRequest @args
    } catch {
        $detail = ""
        if ($_.Exception.Response) {
            $reader = New-Object System.IO.StreamReader($_.Exception.Response.GetResponseStream())
            $detail = $reader.ReadToEnd()
            $reader.Close()
        }
        if (-not $detail) { $detail = $_.Exception.Message }
        throw "$Method $Uri failed: $detail"
    }

    $text = [string]$response.Content
    if ([string]::IsNullOrWhiteSpace($text)) {
        return [pscustomobject]@{ code = "OK"; body = $null; message = $null }
    }
    try {
        return ($text | ConvertFrom-Json)
    } catch {
        return [pscustomobject]@{ code = "OK"; body = $text; message = $null }
    }
}

function Assert-Ok($result, $what) {
    if (-not $result.code -or $result.code -ne "OK") {
        Fail "$what -> code=$($result.code) message=$($result.message)"
    }
}

function New-MultipartBody([string]$filePath) {
    $boundary = "----TeivrimRustore" + [guid]::NewGuid().ToString("N")
    $nl = "`r`n"
    $file = Get-Item $filePath
    $head = @(
        "--$boundary",
        "Content-Disposition: form-data; name=`"file`"; filename=`"$($file.Name)`"",
        "Content-Type: application/vnd.android.package-archive",
        "",
        ""
    ) -join $nl
    $tail = "$nl--$boundary--$nl"

    $headBytes = [System.Text.Encoding]::UTF8.GetBytes($head)
    $fileBytes = [System.IO.File]::ReadAllBytes($file.FullName)
    $tailBytes = [System.Text.Encoding]::UTF8.GetBytes($tail)

    $body = New-Object byte[] ($headBytes.Length + $fileBytes.Length + $tailBytes.Length)
    [Array]::Copy($headBytes, 0, $body, 0, $headBytes.Length)
    [Array]::Copy($fileBytes, 0, $body, $headBytes.Length, $fileBytes.Length)
    [Array]::Copy($tailBytes, 0, $body, $headBytes.Length + $fileBytes.Length, $tailBytes.Length)

    return [pscustomobject]@{
        Body = $body
        ContentType = "multipart/form-data; boundary=$boundary"
    }
}

# ---------------------------------------------------------------- 1. auth

Step "Requesting a Public-Token"

$timestamp = [DateTimeOffset]::UtcNow.ToString("yyyy-MM-ddTHH:mm:ss.fffZ")
$rsa = New-RsaFromPrivateKey $env:RUSTORE_PRIVATE_KEY
try {
    # The signed payload is the key id followed immediately by the timestamp:
    # no separator, no encoding, exactly as concatenated here.
    $payload = [System.Text.Encoding]::UTF8.GetBytes($env:RUSTORE_KEY_ID + $timestamp)
    $raw = $rsa.SignData(
        $payload,
        [System.Security.Cryptography.HashAlgorithmName]::SHA512,
        [System.Security.Cryptography.RSASignaturePadding]::Pkcs1
    )
} finally {
    $rsa.Dispose()
}
$signature = [Convert]::ToBase64String($raw)

$auth = Invoke-Api -Method Post -Uri "$Base/public/auth" `
    -Body @{ keyId = $env:RUSTORE_KEY_ID; timestamp = $timestamp; signature = $signature }
Assert-Ok $auth "auth"
$token = $auth.body.jwe
if ([string]::IsNullOrWhiteSpace($token)) { Fail "auth returned no token" }
Ok "token received ($($token.Length) chars)"

$jsonHeaders = @{ "Public-Token" = $token }

# ----------------------------------------------------------- 2. find a draft

Step "Looking for an existing draft"

$target = $VersionId
if (-not $target) {
    $versions = Invoke-Api -Method Get -Headers $jsonHeaders `
        -Uri "$Base/public/v1/application/$PackageName/version?versionStatuses=DRAFT&size=1"
    Assert-Ok $versions "version list"
    $existing = @($versions.body.content) | Where-Object { $_ } | Select-Object -First 1
    if ($existing) { $target = $existing.versionId }
}

switch ($DraftStrategy) {
    "delete_and_create" {
        if ($target) {
            Step "Deleting draft $target"
            $deleted = Invoke-Api -Method Delete -Headers $jsonHeaders `
                -Uri "$Base/public/v1/application/$PackageName/version/$target"
            Assert-Ok $deleted "delete draft"
            Ok "deleted"
            $target = ""
        }
    }
    "fail_if_exists" {
        if ($target) { Fail "A draft already exists ($target) and the strategy is fail_if_exists." }
    }
    "reuse_existing" {
        if ($target) { Ok "reusing draft $target" }
    }
}

# --------------------------------------------------------- 3. create draft

if (-not $target) {
    Step "Creating a draft"
    $body = [ordered]@{}
    foreach ($property in $metadata.PSObject.Properties) {
        if ($null -ne $property.Value) { $body[$property.Name] = $property.Value }
    }
    $created = Invoke-Api -Method Post -Headers $jsonHeaders `
        -Uri "$Base/public/v1/application/$PackageName/version" -Body $body
    Assert-Ok $created "create draft"
    $target = $created.body
    if (-not $target) { Fail "create draft returned no versionId" }
    Ok "draft $target"
} else {
    Ok "using draft $target"
}

# ------------------------------------------------------------ 4. upload aab

Step "Uploading the bundle"
$multipart = New-MultipartBody $bundle
$upload = Invoke-Api -Method Post -Uri "$Base/public/v1/application/$PackageName/version/$target/aab" `
    -Headers @{ "Public-Token" = $token } `
    -RawBody $multipart.Body -ContentType $multipart.ContentType
Assert-Ok $upload "upload"
Ok "uploaded"

# ---------------------------------------------------------------- 5. commit

if ($Commit) {
    Step "Submitting for moderation"
    $commit = Invoke-Api -Method Post -Headers $jsonHeaders `
        -Uri "$Base/public/v1/application/$PackageName/version/$target/commit"
    Assert-Ok $commit "commit"
    Ok "submitted for review"
} else {
    Write-Host ""
    Write-Host "Uploaded to draft $target. It is NOT submitted for review." -ForegroundColor Yellow
    Write-Host "Finish the listing in the console, then re-run with -Commit, or submit it there."
}

Write-Host ""
Write-Host "Done." -ForegroundColor Green
Write-Host ""
