param(
    [string]$Message,
    [string]$CommitSha = "HEAD",
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

$RootVersion = Get-ProductVersionFromJson ([System.IO.File]::ReadAllText($RootPkgPath))

if (Test-Path -LiteralPath $CargoTomlPath) {
    $CargoVersion = Get-CargoWorkspaceVersionFromToml ([System.IO.File]::ReadAllText($CargoTomlPath))
    if ($CargoVersion -cne $RootVersion) {
        throw "Cargo.toml [workspace.package] version '$CargoVersion' does not match root package.json version '$RootVersion'."
    }
}

if (Test-Path -LiteralPath $TauriConfPath) {
    $TauriVersion = Get-TauriVersionFromJson ([System.IO.File]::ReadAllText($TauriConfPath))
    if ($TauriVersion -cne $RootVersion) {
        throw "src-tauri/tauri.conf.json version '$TauriVersion' does not match root package.json version '$RootVersion'."
    }
}

if (Test-Path -LiteralPath $CoreLibRsPath) {
    $CoreVersion = Get-CoreVersionFromRust ([System.IO.File]::ReadAllText($CoreLibRsPath))
    if ($CoreVersion -cne $RootVersion) {
        throw "crates/uncord-core/src/lib.rs VERSION '$CoreVersion' does not match root package.json version '$RootVersion'."
    }
}

$ResolvedCommit = (& git -C $RepoRoot rev-parse "$CommitSha^{commit}" 2>$null)
if ($LASTEXITCODE -ne 0) { throw "Cannot resolve commit '$CommitSha'" }
$ResolvedCommit = $ResolvedCommit.Trim()
$CommitPkgLines = & git -C $RepoRoot show "${ResolvedCommit}:package.json"
if ($LASTEXITCODE -ne 0) { throw "Cannot read package.json at '$ResolvedCommit'" }
$CommitVersion = Get-ProductVersionFromJson ($CommitPkgLines -join "`n")

if ($Message) {
    $Bump = Get-VersionBump $Message
    $ExpectedVersion = Get-NextProductVersion -Version $CommitVersion -Bump $Bump
    if ($RootVersion -cne $ExpectedVersion) {
        throw "Working tree requires product version $ExpectedVersion ($Bump), but contains $RootVersion."
    }
    $ValidatedVersion = $RootVersion
} else {
    if ($RootVersion -cne $CommitVersion) {
        throw "Working tree version $RootVersion differs from $CommitSha version $CommitVersion. Pass -Message with the proposed full commit message before committing."
    }
    $ParentLine = (& git -C $RepoRoot rev-list --parents -n 1 $ResolvedCommit).Trim()
    if ($LASTEXITCODE -ne 0) { throw "Cannot inspect parents for '$ResolvedCommit'" }
    $ParentTokens = @($ParentLine -split '\s+' | Where-Object { $_ })
    if ($ParentTokens.Count -gt 1) {
        $Parent = $ParentTokens[1]
        $ParentPkgLines = & git -C $RepoRoot show "${Parent}:package.json"
        if ($LASTEXITCODE -ne 0) { throw "Cannot read package.json at parent '$Parent'" }
        $ParentVersion = Get-ProductVersionFromJson ($ParentPkgLines -join "`n")
        $CommitMessage = (& git -C $RepoRoot show -s --format=%B $ResolvedCommit) -join "`n"
        $Bump = Get-VersionBump $CommitMessage
        $ExpectedVersion = Get-NextProductVersion -Version $ParentVersion -Bump $Bump
        if ($CommitVersion -cne $ExpectedVersion) {
            throw "Commit $ResolvedCommit requires product version $ExpectedVersion ($Bump), but contains $CommitVersion."
        }
    }
    $ValidatedVersion = $CommitVersion
    Get-ValidatedVersionHistory -RepoRoot $RepoRoot -TargetSha $ResolvedCommit | Out-Null
}

if ($env:GITHUB_REF_TYPE -eq "tag") {
    $ExpectedTag = "v$ValidatedVersion"
    if ($env:GITHUB_REF_NAME -cne $ExpectedTag) {
        throw "Git tag '$env:GITHUB_REF_NAME' must match product version '$ExpectedTag'."
    }
}

Write-Host "Product version validation passed: $ValidatedVersion" -ForegroundColor Green
