param(
    [Parameter(Mandatory, HelpMessage = "Exact full commit message, including any BREAKING CHANGE body")][ValidateNotNullOrEmpty()][string]$Message,
    [string]$RepoRoot
)

Set-StrictMode -Version 3.0
$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "versioning.ps1")

$RepoRoot = if ($RepoRoot) { $RepoRoot } else { Join-Path $PSScriptRoot ".." }
$RepoRoot = (Resolve-Path $RepoRoot).Path
$RootPkgPath = Join-Path $RepoRoot "package.json"
$CargoTomlPath = Join-Path $RepoRoot "Cargo.toml"
$TauriConfPath = Join-Path $RepoRoot "src-tauri\tauri.conf.json"
$CoreLibRsPath = Join-Path $RepoRoot "crates\uncord-core\src\lib.rs"

$CurrentRootPkg = [System.IO.File]::ReadAllText($RootPkgPath)
$CurrentVersion = Get-ProductVersionFromJson $CurrentRootPkg

$HeadPkgLines = & git -C $RepoRoot show "HEAD:package.json"
if ($LASTEXITCODE -ne 0) {
    throw "Cannot read package.json from HEAD"
}
$HeadVersion = Get-ProductVersionFromJson ($HeadPkgLines -join "`n")
$Bump = Get-VersionBump $Message
$ExpectedVersion = Get-NextProductVersion -Version $HeadVersion -Bump $Bump

if ($CurrentVersion -eq $ExpectedVersion -and $Bump -ne "none") {
    Write-Host "Product version is already prepared: $CurrentVersion"
    return
}
if ($CurrentVersion -ne $HeadVersion) {
    throw "Product version is '$CurrentVersion'; expected HEAD version '$HeadVersion' or prepared version '$ExpectedVersion'."
}
if ($Bump -eq "none") {
    Write-Host "Commit type does not change the product version ($CurrentVersion)."
    return
}

$Snapshots = @{}
$TargetFiles = @($RootPkgPath, $CargoTomlPath, $TauriConfPath, $CoreLibRsPath)
foreach ($Path in $TargetFiles) {
    if (Test-Path -LiteralPath $Path) {
        $Snapshots[$Path] = [System.IO.File]::ReadAllText($Path)
    }
}

$Utf8NoBom = [System.Text.UTF8Encoding]::new($false)
try {
    if ($Snapshots.ContainsKey($RootPkgPath)) {
        $UpdatedPkg = Set-ProductVersionInJson -Content $Snapshots[$RootPkgPath] -Version $ExpectedVersion
        [System.IO.File]::WriteAllText($RootPkgPath, $UpdatedPkg, $Utf8NoBom)
    }

    if ($Snapshots.ContainsKey($CargoTomlPath)) {
        $UpdatedCargo = Set-CargoWorkspaceVersionInToml -Content $Snapshots[$CargoTomlPath] -Version $ExpectedVersion
        [System.IO.File]::WriteAllText($CargoTomlPath, $UpdatedCargo, $Utf8NoBom)
    }

    if ($Snapshots.ContainsKey($TauriConfPath)) {
        $UpdatedTauri = Set-TauriVersionInJson -Content $Snapshots[$TauriConfPath] -Version $ExpectedVersion
        [System.IO.File]::WriteAllText($TauriConfPath, $UpdatedTauri, $Utf8NoBom)
    }

    if ($Snapshots.ContainsKey($CoreLibRsPath)) {
        $UpdatedCore = Set-CoreVersionInRust -Content $Snapshots[$CoreLibRsPath] -Version $ExpectedVersion
        [System.IO.File]::WriteAllText($CoreLibRsPath, $UpdatedCore, $Utf8NoBom)
    }
} catch {
    foreach ($Path in $Snapshots.Keys) {
        [System.IO.File]::WriteAllText($Path, $Snapshots[$Path], $Utf8NoBom)
    }
    throw
}

Write-Host "Prepared product version: $HeadVersion -> $ExpectedVersion ($Bump)" -ForegroundColor Green
