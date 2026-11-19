param(
    [string]$RepoRoot,
    [string]$ArtifactsDir,
    [string]$Version,
    [string]$Repository = "Woffluon/uncord",
    [switch]$RequireSignature
)

Set-StrictMode -Version 3.0
$ErrorActionPreference = "Stop"

$RepoRoot = if ($RepoRoot) { (Resolve-Path $RepoRoot).Path } else { (Resolve-Path (Join-Path $PSScriptRoot "..")).Path }
$ArtifactsDir = if ($ArtifactsDir) { $ArtifactsDir } else { Join-Path $RepoRoot "artifacts" }

if ([string]::IsNullOrWhiteSpace($Version)) {
    $Pkg = Get-Content -LiteralPath (Join-Path $RepoRoot "package.json") -Raw | ConvertFrom-Json
    $Version = [string]$Pkg.version
}
if ($Version -notmatch '^\d+\.\d+\.\d+$') {
    throw "Invalid semver version '$Version'."
}
if (-not (Test-Path -LiteralPath $ArtifactsDir)) {
    throw "Artifacts directory does not exist: $ArtifactsDir"
}

$Tag = "v$Version"
$PortableName = "uncord-$Tag-windows-x64-portable.zip"
$InstallerName = "uncord_${Version}_x64-setup.exe"
$ChecksumsName = "uncord-$Tag-SHA256SUMS.txt"
$ManifestName = "latest.json"
$SignatureName = "latest.json.sig"

$Required = @($PortableName, $InstallerName, $ChecksumsName, $ManifestName)
if ($RequireSignature) {
    $Required += $SignatureName
}

foreach ($FileName in $Required) {
    $Path = Join-Path $ArtifactsDir $FileName
    if (-not (Test-Path -LiteralPath $Path) -or (Get-Item -LiteralPath $Path).Length -le 0) {
        throw "Required release artifact missing or empty: $Path"
    }
}

$ManifestPath = Join-Path $ArtifactsDir $ManifestName
$ManifestBytes = [IO.File]::ReadAllBytes($ManifestPath)
if ($ManifestBytes.Length -ge 3 -and $ManifestBytes[0] -eq 0xEF -and $ManifestBytes[1] -eq 0xBB -and $ManifestBytes[2] -eq 0xBF) {
    throw "latest.json must be UTF-8 without BOM so Ed25519 signatures remain canonical."
}

$ManifestText = [Text.Encoding]::UTF8.GetString($ManifestBytes)
$Manifest = $ManifestText | ConvertFrom-Json

$ExpectedReleaseUrl = "https://github.com/$Repository/releases/tag/$Tag"
$ExpectedInstallerUrl = "https://github.com/$Repository/releases/download/$Tag/$InstallerName"

if ($Manifest.schemaVersion -ne 1) {
    throw "latest.json schemaVersion must be 1 (got $($Manifest.schemaVersion))."
}
if ($Manifest.version -cne $Version) {
    throw "latest.json version mismatch: expected '$Version', got '$($Manifest.version)'."
}
$PublishedAt = [DateTimeOffset]::MinValue
if (-not [DateTimeOffset]::TryParse([string]$Manifest.publishedAt, [Globalization.CultureInfo]::InvariantCulture, [Globalization.DateTimeStyles]::AssumeUniversal, [ref]$PublishedAt)) {
    throw "latest.json publishedAt is not valid ISO-8601 UTC: '$($Manifest.publishedAt)'."
}
if ($Manifest.releaseUrl -cne $ExpectedReleaseUrl) {
    throw "latest.json releaseUrl mismatch: expected '$ExpectedReleaseUrl', got '$($Manifest.releaseUrl)'."
}
if ([string]::IsNullOrWhiteSpace([string]$Manifest.releaseNotes)) {
    throw "latest.json releaseNotes must be non-empty."
}

$InstallerPath = Join-Path $ArtifactsDir $InstallerName
$InstallerSize = [int64](Get-Item -LiteralPath $InstallerPath).Length
$InstallerHash = (Get-FileHash -LiteralPath $InstallerPath -Algorithm SHA256).Hash.ToLowerInvariant()
$PortablePath = Join-Path $ArtifactsDir $PortableName
$PortableHash = (Get-FileHash -LiteralPath $PortablePath -Algorithm SHA256).Hash.ToLowerInvariant()

if ($Manifest.installer.url -cne $ExpectedInstallerUrl) {
    throw "latest.json installer.url mismatch: expected '$ExpectedInstallerUrl', got '$($Manifest.installer.url)'."
}
if ($Manifest.installer.sha256 -cne $InstallerHash) {
    throw "latest.json installer.sha256 mismatch: expected '$InstallerHash', got '$($Manifest.installer.sha256)'."
}
if ([int64]$Manifest.installer.size -ne $InstallerSize) {
    throw "latest.json installer.size mismatch: expected $InstallerSize, got $($Manifest.installer.size)."
}

$ChecksumLines = @(Get-Content -LiteralPath (Join-Path $ArtifactsDir $ChecksumsName) | Where-Object { -not [string]::IsNullOrWhiteSpace($_) })
$ChecksumMap = @{}
foreach ($Line in $ChecksumLines) {
    if ($Line -notmatch '^(?<hash>[a-f0-9]{64})  (?<file>[^\\/]+)$') {
        throw "Invalid checksum line in ${ChecksumsName}: $Line"
    }
    $ChecksumMap[$Matches.file] = $Matches.hash
}
if ($ChecksumMap[$PortableName] -cne $PortableHash -or $ChecksumMap[$InstallerName] -cne $InstallerHash) {
    throw "$ChecksumsName hashes do not match staged artifacts."
}

if ($RequireSignature) {
    $SignaturePath = Join-Path $ArtifactsDir $SignatureName
    & (Join-Path $RepoRoot "scripts\update-manifest-signature.ps1") `
        -Action Verify `
        -ManifestPath $ManifestPath `
        -SignaturePath $SignaturePath
    if ($LASTEXITCODE -ne 0) {
        throw "Signature verification failed for $ManifestPath."
    }
}

Write-Host "Release manifest validation PASSED for v$Version (signed=$RequireSignature)." -ForegroundColor Green
