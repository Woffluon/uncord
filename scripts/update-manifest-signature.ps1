param(
    [Parameter(Mandatory)]
    [ValidateSet("CheckKey", "Sign", "Verify")]
    [string]$Action,
    [string]$ManifestPath,
    [string]$SignaturePath,
    [string]$PrivateKeyBase64 = $env:UPDATE_MANIFEST_PRIVATE_KEY_BASE64,
    [string]$PublicKeyPath
)

Set-StrictMode -Version 3.0
$ErrorActionPreference = "Stop"

$ScriptDir = if ($PSScriptRoot) { $PSScriptRoot } else { Split-Path -Parent $MyInvocation.MyCommand.Path }
$PublicKeyPath = if ($PublicKeyPath) { $PublicKeyPath } else { Join-Path $ScriptDir "..\assets\update-public-key.hex" }

$Ed25519SpkiHeader = [byte[]](0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00)

function Find-OpenSsl {
    $Command = Get-Command openssl -ErrorAction SilentlyContinue
    if ($Command) { return $Command.Source }
    foreach ($Candidate in @(
        "C:\Program Files\Git\usr\bin\openssl.exe",
        "C:\Program Files\Git\mingw64\bin\openssl.exe"
    )) {
        if (Test-Path -LiteralPath $Candidate) { return $Candidate }
    }
    throw "OpenSSL 3 with Ed25519 support is required."
}

function ConvertFrom-HexString([string]$Hex) {
    $Clean = ($Hex.Trim() -replace '\s+', '').ToLowerInvariant()
    if ($Clean -notmatch '^[0-9a-f]{64}$') {
        throw "Update public key must be 64 hexadecimal characters (32 raw Ed25519 bytes)."
    }
    $Bytes = New-Object byte[] 32
    for ($Index = 0; $Index -lt 32; $Index++) {
        $Bytes[$Index] = [Convert]::ToByte($Clean.Substring($Index * 2, 2), 16)
    }
    return $Bytes
}

function Write-PemFile([string]$Path, [string]$Label, [byte[]]$DerBytes) {
    $Base64 = [Convert]::ToBase64String($DerBytes, [Base64FormattingOptions]::InsertLineBreaks)
    $Content = "-----BEGIN $Label-----`n$Base64`n-----END $Label-----`n"
    [IO.File]::WriteAllText($Path, $Content, [Text.UTF8Encoding]::new($false))
}

function Read-ExpectedPublicKey([string]$Path) {
    if (-not (Test-Path -LiteralPath $Path)) {
        throw "Update public key file not found at '$Path'."
    }
    return ConvertFrom-HexString ([IO.File]::ReadAllText($Path))
}

function Write-ExpectedPublicPem([string]$Path, [byte[]]$PublicKeyBytes) {
    $Spki = New-Object byte[] ($Ed25519SpkiHeader.Length + $PublicKeyBytes.Length)
    [Buffer]::BlockCopy($Ed25519SpkiHeader, 0, $Spki, 0, $Ed25519SpkiHeader.Length)
    [Buffer]::BlockCopy($PublicKeyBytes, 0, $Spki, $Ed25519SpkiHeader.Length, $PublicKeyBytes.Length)
    Write-PemFile $Path "PUBLIC KEY" $Spki
}

function Write-PrivatePemFromSecret([string]$Path, [string]$SecretBase64) {
    if ([string]::IsNullOrWhiteSpace($SecretBase64)) {
        throw "UPDATE_MANIFEST_PRIVATE_KEY_BASE64 is required for signing."
    }
    try {
        $Bytes = [Convert]::FromBase64String($SecretBase64.Trim())
    } catch {
        throw "UPDATE_MANIFEST_PRIVATE_KEY_BASE64 is not valid base64."
    }
    # If the base64 secret is already a PEM-encoded string, write it directly.
    # Otherwise if it is raw PKCS#8 DER bytes, wrap it in PEM headers.
    $IsPem = ($Bytes.Length -ge 5 -and $Bytes[0] -eq 0x2D -and $Bytes[1] -eq 0x2D -and $Bytes[2] -eq 0x2D -and $Bytes[3] -eq 0x2D -and $Bytes[4] -eq 0x2D)
    if ($IsPem) {
        [IO.File]::WriteAllBytes($Path, $Bytes)
    } else {
        Write-PemFile $Path "PRIVATE KEY" $Bytes
    }
}

function Invoke-OpenSsl([string]$OpenSsl, [string[]]$Arguments) {
    $Output = & $OpenSsl @Arguments 2>&1
    if ($LASTEXITCODE -ne 0) {
        $Text = ($Output | ForEach-Object { "$_" }) -join "`n"
        throw "OpenSSL failed ($($Arguments -join ' ')): $Text"
    }
}

$OpenSsl = Find-OpenSsl
$ExpectedPublic = Read-ExpectedPublicKey $PublicKeyPath
$WorkDir = Join-Path ([IO.Path]::GetTempPath()) ("uncord-ed25519-" + [Guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $WorkDir -Force | Out-Null

try {
    $PublicPem = Join-Path $WorkDir "public.pem"
    Write-ExpectedPublicPem $PublicPem $ExpectedPublic

    if ($Action -in @("CheckKey", "Sign")) {
        $PrivatePem = Join-Path $WorkDir "private.pem"
        Write-PrivatePemFromSecret $PrivatePem $PrivateKeyBase64
        $DerivedPublicDer = Join-Path $WorkDir "derived-public.der"
        Invoke-OpenSsl $OpenSsl @("pkey", "-in", $PrivatePem, "-pubout", "-outform", "DER", "-out", $DerivedPublicDer)
        $DerivedSpki = [IO.File]::ReadAllBytes($DerivedPublicDer)
        if ($DerivedSpki.Length -ne 44) {
            throw "Signing key is not an Ed25519 PKCS#8 private key."
        }
        $HeaderMatches = $true
        for ($i = 0; $i -lt $Ed25519SpkiHeader.Length; $i++) {
            if ($DerivedSpki[$i] -ne $Ed25519SpkiHeader[$i]) {
                $HeaderMatches = $false
                break
            }
        }
        if (-not $HeaderMatches) {
            throw "Derived SPKI header does not match Ed25519."
        }
        $DerivedHex = ([BitConverter]::ToString($DerivedSpki[12..43])).Replace("-", "").ToLowerInvariant()
        $ExpectedHex = ([BitConverter]::ToString($ExpectedPublic)).Replace("-", "").ToLowerInvariant()
        if ($DerivedHex -cne $ExpectedHex) {
            throw "Signing private key does not match pinned assets/update-public-key.hex ($ExpectedHex)."
        }
    }

    if ($Action -eq "CheckKey") {
        Write-Host "Update signing key matches assets/update-public-key.hex." -ForegroundColor Green
        return
    }

    if ([string]::IsNullOrWhiteSpace($ManifestPath) -or -not (Test-Path -LiteralPath $ManifestPath)) {
        throw "Manifest file not found: '$ManifestPath'"
    }
    $ResolvedManifest = (Resolve-Path $ManifestPath).Path
    $TargetSig = if ($SignaturePath) { $SignaturePath } else { "$ResolvedManifest.sig" }

    if ($Action -eq "Sign") {
        $PrivatePem = Join-Path $WorkDir "private.pem"
        $TempSig = Join-Path $WorkDir "manifest.sig"
        Invoke-OpenSsl $OpenSsl @("pkeyutl", "-sign", "-inkey", $PrivatePem, "-rawin", "-in", $ResolvedManifest, "-out", $TempSig)
        Invoke-OpenSsl $OpenSsl @("pkeyutl", "-verify", "-pubin", "-inkey", $PublicPem, "-rawin", "-in", $ResolvedManifest, "-sigfile", $TempSig)
        $SigBytes = [IO.File]::ReadAllBytes($TempSig)
        if ($SigBytes.Length -ne 64) {
            throw "Generated Ed25519 signature has invalid length ($($SigBytes.Length) bytes)."
        }
        [IO.File]::WriteAllBytes($TargetSig, $SigBytes)
        Write-Host "Signed update manifest: $TargetSig" -ForegroundColor Green
        return
    }

    if (-not (Test-Path -LiteralPath $TargetSig)) {
        throw "Signature file not found: '$TargetSig'"
    }
    $ResolvedSig = (Resolve-Path $TargetSig).Path
    $SigBytes = [IO.File]::ReadAllBytes($ResolvedSig)
    if ($SigBytes.Length -ne 64) {
        throw "Update manifest signature must be 64 raw bytes (got $($SigBytes.Length))."
    }
    Invoke-OpenSsl $OpenSsl @("pkeyutl", "-verify", "-pubin", "-inkey", $PublicPem, "-rawin", "-in", $ResolvedManifest, "-sigfile", $ResolvedSig)
    Write-Host "Verified update manifest signature: $ResolvedSig" -ForegroundColor Green
} finally {
    if (Test-Path -LiteralPath $WorkDir) {
        Remove-Item -LiteralPath $WorkDir -Recurse -Force -ErrorAction SilentlyContinue
    }
}
