param(
    [string]$RepoRoot,
    [string]$OutDir,
    [switch]$SkipBuild
)

Set-StrictMode -Version 3.0
$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "versioning.ps1")

$RepoRoot = if ($RepoRoot) { (Resolve-Path $RepoRoot).Path } else { (Resolve-Path (Join-Path $PSScriptRoot "..")).Path }
$OutDir = if ($OutDir) { $OutDir } else { Join-Path $RepoRoot "artifacts" }
if (-not (Test-Path -LiteralPath $OutDir)) {
    New-Item -ItemType Directory -Path $OutDir -Force | Out-Null
}
$OutDir = (Resolve-Path $OutDir).Path

$RootPkgPath = Join-Path $RepoRoot "package.json"
$Version = Get-ProductVersionFromJson ([System.IO.File]::ReadAllText($RootPkgPath))

function Resolve-GuiBinary([string]$Root) {
    $Candidates = @(
        (Join-Path $Root "src-tauri\target\release\uncord.exe"),
        (Join-Path $Root "target\release\uncord.exe")
    )
    foreach ($Candidate in $Candidates) {
        if (Test-Path -LiteralPath $Candidate) {
            return $Candidate
        }
    }
    return $null
}

$CliExePath = Join-Path $RepoRoot "target\release\uncord-cli.exe"
$GuiExePath = Resolve-GuiBinary $RepoRoot

if (-not (Test-Path -LiteralPath $CliExePath)) {
    if ($SkipBuild) {
        throw "Missing release CLI binary at '$CliExePath' while -SkipBuild was specified."
    }
    & cargo build --manifest-path (Join-Path $RepoRoot "Cargo.toml") --release -p uncord-cli
    if ($LASTEXITCODE -ne 0) { throw "Failed to build uncord-cli release binary." }
}

if (-not $GuiExePath) {
    if ($SkipBuild) {
        throw "Missing release GUI binary (src-tauri/target/release/uncord.exe or target/release/uncord.exe) while -SkipBuild was specified."
    }
    & pnpm --dir $RepoRoot tauri build
    if ($LASTEXITCODE -ne 0) { throw "Failed to build uncord Tauri desktop application." }
    $GuiExePath = Resolve-GuiBinary $RepoRoot
}

if (-not (Test-Path -LiteralPath $CliExePath)) {
    throw "Expected CLI binary was not found: $CliExePath"
}
if (-not $GuiExePath -or -not (Test-Path -LiteralPath $GuiExePath)) {
    throw "Expected GUI binary (uncord.exe) was not found in src-tauri/target/release or target/release."
}

$ReadmePath = Join-Path $RepoRoot "README.md"
$LicensePath = Join-Path $RepoRoot "LICENSE"
if (-not (Test-Path -LiteralPath $ReadmePath)) { throw "Missing README.md at '$ReadmePath'" }
if (-not (Test-Path -LiteralPath $LicensePath)) { throw "Missing LICENSE at '$LicensePath'" }

Get-ChildItem -LiteralPath $OutDir -Filter "uncord-*.zip" -ErrorAction SilentlyContinue | Remove-Item -Force
Get-ChildItem -LiteralPath $OutDir -Filter "uncord*.exe" -ErrorAction SilentlyContinue | Remove-Item -Force
Get-ChildItem -LiteralPath $OutDir -Filter "uncord-*-SHA256SUMS.txt" -ErrorAction SilentlyContinue | Remove-Item -Force

$PortableZipName = "uncord-v$Version-windows-x64-portable.zip"
$PortableZipPath = Join-Path $OutDir $PortableZipName
$StagingDir = Join-Path ([IO.Path]::GetTempPath()) ("uncord-stage-" + [Guid]::NewGuid().ToString("N"))

try {
    New-Item -ItemType Directory -Path $StagingDir -Force | Out-Null
    Copy-Item -LiteralPath $GuiExePath -Destination (Join-Path $StagingDir "uncord.exe") -Force
    Copy-Item -LiteralPath $CliExePath -Destination (Join-Path $StagingDir "uncord-cli.exe") -Force
    Copy-Item -LiteralPath $ReadmePath -Destination (Join-Path $StagingDir "README.md") -Force
    Copy-Item -LiteralPath $LicensePath -Destination (Join-Path $StagingDir "LICENSE") -Force

    Add-Type -AssemblyName System.IO.Compression.FileSystem
    [System.IO.Compression.ZipFile]::CreateFromDirectory(
        $StagingDir,
        $PortableZipPath,
        [System.IO.Compression.CompressionLevel]::Optimal,
        $false
    )
} finally {
    if (Test-Path -LiteralPath $StagingDir) {
        Remove-Item -LiteralPath $StagingDir -Recurse -Force
    }
}

if (-not (Test-Path -LiteralPath $PortableZipPath)) {
    throw "Expected portable archive was not created: $PortableZipPath"
}

$NsisDirs = @(
    (Join-Path $RepoRoot "src-tauri\target\release\bundle\nsis"),
    (Join-Path $RepoRoot "target\release\bundle\nsis")
)
$CopiedSetupNames = @{}
foreach ($NsisDir in $NsisDirs) {
    if (Test-Path -LiteralPath $NsisDir) {
        $Installers = @(Get-ChildItem -LiteralPath $NsisDir -Filter "uncord_${Version}_*.exe" -File | Sort-Object LastWriteTimeUtc -Descending)
        foreach ($Installer in $Installers) {
            if (-not $CopiedSetupNames.ContainsKey($Installer.Name)) {
                Copy-Item -LiteralPath $Installer.FullName -Destination (Join-Path $OutDir $Installer.Name) -Force
                $CopiedSetupNames[$Installer.Name] = $true
            }
        }
    }
}

$StagedFiles = @(
    Get-ChildItem -LiteralPath $OutDir -File |
        Where-Object { $_.Name -notlike "*-SHA256SUMS.txt" -and $_.Name -ne "latest.json" -and $_.Name -ne "latest.json.sig" } |
        Sort-Object Name
)
if ($StagedFiles.Count -eq 0) {
    throw "No release artifacts were staged in $OutDir"
}

$ChecksumLines = [System.Collections.Generic.List[string]]::new()
foreach ($File in $StagedFiles) {
    $Hash = (Get-FileHash -LiteralPath $File.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
    $ChecksumLines.Add("$Hash  $($File.Name)")
}

$ChecksumFile = Join-Path $OutDir "uncord-v$Version-SHA256SUMS.txt"
$Utf8NoBom = [System.Text.UTF8Encoding]::new($false)
[System.IO.File]::WriteAllText($ChecksumFile, (($ChecksumLines -join "`n") + "`n"), $Utf8NoBom)

$InstallerName = "uncord_${Version}_x64-setup.exe"
$InstallerStagedPath = Join-Path $OutDir $InstallerName
if (Test-Path -LiteralPath $InstallerStagedPath) {
    $InstallerItem = Get-Item -LiteralPath $InstallerStagedPath
    $InstallerHash = (Get-FileHash -LiteralPath $InstallerStagedPath -Algorithm SHA256).Hash.ToLowerInvariant()
    $Tag = "v$Version"
    $ReleaseRepo = if (-not [string]::IsNullOrWhiteSpace($env:GITHUB_REPOSITORY)) { $env:GITHUB_REPOSITORY } else { "Woffluon/uncord" }
    $ManifestObject = [ordered]@{
        schemaVersion = 1
        version       = $Version
        publishedAt   = [DateTimeOffset]::UtcNow.ToString("yyyy-MM-ddTHH:mm:ssZ")
        releaseUrl    = "https://github.com/$ReleaseRepo/releases/tag/$Tag"
        releaseNotes  = "uncord $Tag release"
        installer     = [ordered]@{
            url    = "https://github.com/$ReleaseRepo/releases/download/$Tag/$InstallerName"
            sha256 = $InstallerHash
            size   = [int64]$InstallerItem.Length
        }
    }
    $ManifestJson = ($ManifestObject | ConvertTo-Json -Depth 4) -replace "`r`n", "`n"
    [System.IO.File]::WriteAllText((Join-Path $OutDir "latest.json"), "$ManifestJson`n", $Utf8NoBom)
}

Write-Host "Staged release artifacts in ${OutDir}:" -ForegroundColor Green
foreach ($Line in $ChecksumLines) {
    Write-Host "  $Line"
}
if (Test-Path -LiteralPath (Join-Path $OutDir "latest.json")) {
    Write-Host "  manifest: latest.json" -ForegroundColor Green
}
