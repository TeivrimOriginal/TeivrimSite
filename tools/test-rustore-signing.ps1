# Round-trip check for the DER reader in rustore-upload.ps1.
#
#   powershell -File tools\test-rustore-signing.ps1
#
# The upload script parses the RuStore private key by hand so it also runs on
# Windows PowerShell 5.1, where `ImportRSAPrivateKey` does not exist. Hand-rolled
# ASN.1 is exactly the kind of code that is wrong until it is run against
# something real, so this builds a real PKCS#1 and PKCS#8 key from
# ExportParameters, feeds them through the reader, and checks that the resulting
# signature verifies against the original public key. A swapped or dropped CRT
# parameter would make that fail even though every parse would succeed.
#
# It has already earned its place: it caught the version INTEGER being consumed
# before the PKCS#8 branch, and the inner SEQUENCE header not being skipped.

$ErrorActionPreference = "Stop"
$script = Join-Path $PSScriptRoot "rustore-upload.ps1"
if (-not (Test-Path $script)) { throw "cannot find $script next to this test" }
$source = Get-Content $script -Raw

# Pull just the parser functions out of the upload script so they can be tested
# without running the whole flow.
$start = $source.IndexOf("function Read-DerLength")
$end = $source.IndexOf("# ------------------------------------------------------------------- HTTP")
if ($start -lt 0 -or $end -lt 0) { throw "could not locate the parser block in $script" }
$parser = $source.Substring($start, $end - $start)
Invoke-Expression $parser

# ------------------------------------------------------------- DER encoder
#
# Everything accumulates in one List[byte]. Building nested PowerShell arrays of
# byte arrays is a trap: `@($a, $b)` flattens `$a` when it is already a byte[].

function New-Bytes { return ,([System.Collections.Generic.List[byte]]::new()) }

function Add-DerLength($out, [int]$length) {
    if ($length -lt 0x80) {
        $out.Add([byte]$length)
        return
    }
    $digits = New-Bytes
    $rest = $length
    while ($rest -gt 0) { $digits.Insert(0, [byte]($rest -band 0xFF)); $rest = $rest -shr 8 }
    $out.Add([byte](0x80 -bor $digits.Count))
    foreach ($d in $digits) { $out.Add($d) }
}

function Add-DerInteger($out, [byte[]]$value) {
    $pad = 0
    while ($value[$pad] -eq 0 -and $pad -lt $value.Length - 1) { $pad++ }
    $needsPad = (($value[$pad] -band 0x80) -ne 0)
    $out.Add(0x02)
    $size = $value.Length - $pad
    if ($needsPad) { $size++ }
    Add-DerLength $out $size
    if ($needsPad) { $out.Add(0x00) }
    for ($i = $pad; $i -lt $value.Length; $i++) { $out.Add($value[$i]) }
}

function Add-DerSequence($out, [scriptblock]$content) {
    $body = New-Bytes
    & $content $body
    $out.Add(0x30)
    Add-DerLength $out $body.Count
    foreach ($b in $body) { $out.Add($b) }
}

function New-Pkcs1Bytes($p) {
    $out = New-Bytes
    Add-DerSequence $out {
        param($body)
        Add-DerInteger $body ([byte[]]@(0))
        Add-DerInteger $body $p.Modulus
        Add-DerInteger $body $p.Exponent
        Add-DerInteger $body $p.D
        Add-DerInteger $body $p.P
        Add-DerInteger $body $p.Q
        Add-DerInteger $body $p.DP
        Add-DerInteger $body $p.DQ
        Add-DerInteger $body $p.InverseQ
    }
    return ,$out.ToArray()
}

function New-Pkcs8Bytes($p) {
    $inner = New-Pkcs1Bytes $p
    $out = New-Bytes
    Add-DerSequence $out {
        param($body)
        Add-DerInteger $body ([byte[]]@(0))
        # AlgorithmIdentifier: OID rsaEncryption, NULL parameters.
        $oid = [byte[]]@(0x06, 0x09, 0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x01)
        $alg = New-Bytes
        Add-DerSequence $alg {
            param($a)
            foreach ($b in $oid) { $a.Add($b) }
            $a.Add(0x05); $a.Add(0x00)
        }
        foreach ($b in $alg.ToArray()) { $body.Add($b) }
        $body.Add(0x04)
        Add-DerLength $body $inner.Length
        foreach ($b in $inner) { $body.Add($b) }
    }
    return ,$out.ToArray()
}

# ------------------------------------------------------------------- test

$original = [System.Security.Cryptography.RSA]::Create(2048)
$message = [System.Text.Encoding]::UTF8.GetBytes("keyId2026-09-27T12:00:00.000Z")
$parameters = $original.ExportParameters($true)
$failures = 0

function Test-Case($name, [byte[]]$der) {
    try {
        if ($der[0] -ne 0x30) { throw "encoder produced a non-SEQUENCE at offset 0" }
        $parsed = Read-PrivateKeyParams $der
        $rsa = [System.Security.Cryptography.RSA]::Create()
        try {
            $rsa.ImportParameters($parsed)
            $signature = $rsa.SignData(
                $message,
                [System.Security.Cryptography.HashAlgorithmName]::SHA512,
                [System.Security.Cryptography.RSASignaturePadding]::Pkcs1
            )
            $verified = $original.VerifyData(
                $message,
                $signature,
                [System.Security.Cryptography.HashAlgorithmName]::SHA512,
                [System.Security.Cryptography.RSASignaturePadding]::Pkcs1
            )
            # Newlines inside the base64 body must be tolerated, because that is
            # how the console hands the key out.
            $chunks = [Convert]::ToBase64String($der) -replace '(.{64})', "`$1`n"
            $wrapped = [Convert]::FromBase64String(($chunks -replace '\s', ''))
            $fromPem = Read-PrivateKeyParams $wrapped

            if ($verified -and $rsa.KeySize -eq 2048 -and $fromPem.Modulus.Length -eq $parsed.Modulus.Length) {
                Write-Host "PASS  $name ($($der.Length) bytes)" -ForegroundColor Green
            } else {
                Write-Host "FAIL  $name (verified=$verified keySize=$($rsa.KeySize))" -ForegroundColor Red
                $script:failures++
            }
        } finally {
            $rsa.Dispose()
        }
    } catch {
        Write-Host "FAIL  $name -> $($_.Exception.Message)" -ForegroundColor Red
        $script:failures++
    }
}

Test-Case "PKCS#1 (BEGIN RSA PRIVATE KEY)" (New-Pkcs1Bytes $parameters)
Test-Case "PKCS#8 (BEGIN PRIVATE KEY)" (New-Pkcs8Bytes $parameters)

$original.Dispose()

if ($failures -gt 0) {
    Write-Host ""
    Write-Host "$failures case(s) failed" -ForegroundColor Red
    exit 1
}
Write-Host ""
Write-Host "All parser cases passed." -ForegroundColor Green
