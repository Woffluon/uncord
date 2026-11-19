Set-StrictMode -Version 3.0
$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "versioning.ps1")

function Assert-Equal($Expected, $Actual, [string]$Case) {
    if ($Expected -cne $Actual) {
        throw "$Case failed: expected '$Expected', got '$Actual'"
    }
}

Assert-Equal "minor" (Get-VersionBump "feat: add multi-route WASAPI mixer") "feat bump"
Assert-Equal "minor" (Get-VersionBump "feat(core): add adaptive Hermite resampler") "scoped feat bump"
Assert-Equal "patch" (Get-VersionBump "fix: prevent buffer underrun on hot-plug") "fix bump"
Assert-Equal "patch" (Get-VersionBump "perf(dsp): pre-allocate SPSC ring buffer") "perf bump"
Assert-Equal "patch" (Get-VersionBump "refactor: extract WASAPI endpoint helper") "refactor bump"
Assert-Equal "patch" (Get-VersionBump "build: update tauri bundle config") "build bump"
Assert-Equal "patch" (Get-VersionBump "security: restrict IPC capabilities") "security bump"
Assert-Equal "none" (Get-VersionBump "docs: update bilingual README") "docs bump"
Assert-Equal "none" (Get-VersionBump "ci: pin workflow action SHAs") "ci bump"
Assert-Equal "none" (Get-VersionBump "test: add clock-drift benchmark") "test bump"
Assert-Equal "none" (Get-VersionBump "chore: format Rust workspace") "chore bump"
Assert-Equal "none" (Get-VersionBump "chore(deps): bump ringbuf from 0.4.0 to 0.4.1") "deps chore bump"
Assert-Equal "none" (Get-VersionBump "build(deps-dev): bump vite") "deps-dev build bump"
Assert-Equal "major" (Get-VersionBump "feat!: change IPC route schema") "breaking header bump"
Assert-Equal "major" (Get-VersionBump "fix: rename CLI flag`n`nBREAKING CHANGE: --device renamed to --source") "breaking body bump"

Assert-Equal "0.1.1" (Get-NextProductVersion -Version "0.1.0" -Bump "patch") "patch increment"
Assert-Equal "0.2.0" (Get-NextProductVersion -Version "0.1.9" -Bump "minor") "minor increment"
Assert-Equal "1.0.0" (Get-NextProductVersion -Version "0.4.2" -Bump "major") "major increment"
Assert-Equal "0.4.2" (Get-NextProductVersion -Version "0.4.2" -Bump "none") "no increment"

$EmptySelection = @(Get-LatestReleaseSelection -Candidates @() -TargetSha "deadbeef")
Assert-Equal 0 $EmptySelection.Count "empty release selection"
$LatestSelection = @(Get-LatestReleaseSelection -Candidates @(
    [pscustomobject]@{ sha = "1111111"; version = "0.1.1"; tag = "v0.1.1" },
    [pscustomobject]@{ sha = "2222222"; version = "0.2.0"; tag = "v0.2.0" }
) -TargetSha "3333333")
Assert-Equal 1 $LatestSelection.Count "collapsed release selection count"
Assert-Equal "3333333" $LatestSelection[0].sha "collapsed release selection sha"
Assert-Equal "0.2.0" $LatestSelection[0].version "collapsed release selection version"
Assert-Equal "v0.2.0" $LatestSelection[0].tag "collapsed release selection tag"

$SampleCargo = "[workspace]`nmembers = [`"crates/uncord-core`"]`n`n[workspace.package]`nversion = `"0.1.0`"`nedition = `"2021`"`n`n[profile.release]`nopt-level = 3`n"
Assert-Equal "0.1.0" (Get-CargoWorkspaceVersionFromToml $SampleCargo) "read Cargo.toml version"
Assert-Equal "0.2.0" (Get-CargoWorkspaceVersionFromToml (Set-CargoWorkspaceVersionInToml -Content $SampleCargo -Version "0.2.0")) "write Cargo.toml version"

$SampleRust = "pub const VERSION: &str = `"0.1.0`";`npub mod engine;`n"
Assert-Equal "0.1.0" (Get-CoreVersionFromRust $SampleRust) "read uncord-core VERSION"
Assert-Equal "0.2.0" (Get-CoreVersionFromRust (Set-CoreVersionInRust -Content $SampleRust -Version "0.2.0")) "write uncord-core VERSION"

$PreparePath = Join-Path $PSScriptRoot "prepare-version.ps1"
$ValidatePath = Join-Path $PSScriptRoot "validate-version.ps1"
$TestRoot = Join-Path ([IO.Path]::GetTempPath()) ("uncord-version-test-" + [Guid]::NewGuid().ToString("N"))

function Write-TestFile([string]$Path, [string]$Content) {
    $Parent = Split-Path -Parent $Path
    if ($Parent -and -not (Test-Path -LiteralPath $Parent)) {
        New-Item -ItemType Directory -Path $Parent -Force | Out-Null
    }
    [IO.File]::WriteAllText($Path, $Content, [Text.UTF8Encoding]::new($false))
}

function Initialize-TestRepo([string]$RepoPath) {
    & git -C $RepoPath init --quiet
    if ($LASTEXITCODE -ne 0) { throw "Failed to initialize test git repository." }
    & git -C $RepoPath config core.autocrlf false
    & git -C $RepoPath add .
    if ($LASTEXITCODE -ne 0) { throw "Failed to stage test git repository." }
    & git -C $RepoPath -c user.name=uncord-Test -c user.email=test@example.invalid commit --quiet --no-gpg-sign -m "chore: initial version fixture"
    if ($LASTEXITCODE -ne 0) { throw "Failed to commit test git repository." }
}

try {
    $SuccessRepo = Join-Path $TestRoot "success"
    New-Item -ItemType Directory -Path $SuccessRepo | Out-Null
    Write-TestFile (Join-Path $SuccessRepo "package.json") "{`n  `"name`": `"uncord`",`n  `"version`": `"0.1.0`"`n}`n"
    Write-TestFile (Join-Path $SuccessRepo "Cargo.toml") "[workspace]`nresolver = `"2`"`nmembers = [`"crates/uncord-core`", `"crates/uncord-cli`", `"src-tauri`"]`n`n[workspace.package]`nversion = `"0.1.0`"`nedition = `"2021`"`n"
    Write-TestFile (Join-Path $SuccessRepo "src-tauri\tauri.conf.json") "{`n  `"productName`": `"uncord`",`n  `"version`": `"0.1.0`"`n}`n"
    Write-TestFile (Join-Path $SuccessRepo "crates\uncord-core\src\lib.rs") "pub const VERSION: &str = `"0.1.0`";`npub mod backend;`n"
    Initialize-TestRepo $SuccessRepo

    & $PreparePath -Message "feat: add multi-route mixer" -RepoRoot $SuccessRepo
    Assert-Equal "0.2.0" (Get-ProductVersionFromJson ([IO.File]::ReadAllText((Join-Path $SuccessRepo "package.json")))) "prepare package.json version"
    Assert-Equal "0.2.0" (Get-CargoWorkspaceVersionFromToml ([IO.File]::ReadAllText((Join-Path $SuccessRepo "Cargo.toml")))) "prepare Cargo.toml version"
    Assert-Equal "0.2.0" (Get-TauriVersionFromJson ([IO.File]::ReadAllText((Join-Path $SuccessRepo "src-tauri\tauri.conf.json")))) "prepare tauri.conf.json version"
    Assert-Equal "0.2.0" (Get-CoreVersionFromRust ([IO.File]::ReadAllText((Join-Path $SuccessRepo "crates\uncord-core\src\lib.rs")))) "prepare uncord-core VERSION"
    & $ValidatePath -Message "feat: add multi-route mixer" -RepoRoot $SuccessRepo

    $PreparedRoot = [IO.File]::ReadAllText((Join-Path $SuccessRepo "package.json"))
    & $PreparePath -Message "feat: add multi-route mixer" -RepoRoot $SuccessRepo
    Assert-Equal $PreparedRoot ([IO.File]::ReadAllText((Join-Path $SuccessRepo "package.json"))) "prepare idempotency"

    $PreparedCargo = [IO.File]::ReadAllText((Join-Path $SuccessRepo "Cargo.toml"))
    Write-TestFile (Join-Path $SuccessRepo "Cargo.toml") (Set-CargoWorkspaceVersionInToml -Content $PreparedCargo -Version "0.9.9")
    $CargoDriftFailed = $false
    try { & $ValidatePath -Message "feat: add multi-route mixer" -RepoRoot $SuccessRepo } catch { $CargoDriftFailed = $true }
    if (-not $CargoDriftFailed) { throw "validate-version accepted drifted Cargo.toml version" }
    Write-TestFile (Join-Path $SuccessRepo "Cargo.toml") $PreparedCargo

    $PreparedCore = [IO.File]::ReadAllText((Join-Path $SuccessRepo "crates\uncord-core\src\lib.rs"))
    Write-TestFile (Join-Path $SuccessRepo "crates\uncord-core\src\lib.rs") (Set-CoreVersionInRust -Content $PreparedCore -Version "0.9.9")
    $CoreDriftFailed = $false
    try { & $ValidatePath -Message "feat: add multi-route mixer" -RepoRoot $SuccessRepo } catch { $CoreDriftFailed = $true }
    if (-not $CoreDriftFailed) { throw "validate-version accepted drifted uncord-core VERSION" }
    Write-TestFile (Join-Path $SuccessRepo "crates\uncord-core\src\lib.rs") $PreparedCore

    $PreparedTauri = [IO.File]::ReadAllText((Join-Path $SuccessRepo "src-tauri\tauri.conf.json"))
    Write-TestFile (Join-Path $SuccessRepo "src-tauri\tauri.conf.json") (Set-TauriVersionInJson -Content $PreparedTauri -Version "0.9.9")
    $TauriDriftFailed = $false
    try { & $ValidatePath -Message "feat: add multi-route mixer" -RepoRoot $SuccessRepo } catch { $TauriDriftFailed = $true }
    if (-not $TauriDriftFailed) { throw "validate-version accepted drifted tauri.conf.json version" }
    Write-TestFile (Join-Path $SuccessRepo "src-tauri\tauri.conf.json") $PreparedTauri

    Write-TestFile (Join-Path $SuccessRepo "package.json") (Set-ProductVersionInJson -Content $PreparedRoot -Version "0.3.0")
    $ConflictFailed = $false
    try { & $PreparePath -Message "fix: conflicting version" -RepoRoot $SuccessRepo } catch { $ConflictFailed = $true }
    if (-not $ConflictFailed) { throw "prepare version accepted a conflicting product version" }

    Write-TestFile (Join-Path $SuccessRepo "package.json") $PreparedRoot
    Write-TestFile (Join-Path $SuccessRepo "scripts\versioning.ps1") ([IO.File]::ReadAllText((Join-Path $PSScriptRoot "versioning.ps1")))
    & git -C $SuccessRepo add .
    & git -C $SuccessRepo -c user.name=uncord-Test -c user.email=test@example.invalid commit --quiet --no-gpg-sign -m "feat: add multi-route mixer"
    if ($LASTEXITCODE -ne 0) { throw "Failed to commit prepared test version." }

    & $ValidatePath -RepoRoot $SuccessRepo
    $History = @(Get-ValidatedVersionHistory -RepoRoot $SuccessRepo -TargetSha HEAD)
    Assert-Equal 1 $History.Count "version history count"
    Assert-Equal "0.2.0" $History[0].version "version history release"

    $RootContractRepo = Join-Path $TestRoot "root-contract"
    New-Item -ItemType Directory -Path $RootContractRepo | Out-Null
    Write-TestFile (Join-Path $RootContractRepo "package.json") "{`n  `"name`": `"uncord`",`n  `"version`": `"0.1.0`"`n}`n"
    Write-TestFile (Join-Path $RootContractRepo "scripts\versioning.ps1") ([IO.File]::ReadAllText((Join-Path $PSScriptRoot "versioning.ps1")))
    & git -C $RootContractRepo init --quiet
    & git -C $RootContractRepo config core.autocrlf false
    & git -C $RootContractRepo add .
    & git -C $RootContractRepo -c user.name=uncord-Test -c user.email=test@example.invalid commit --quiet --no-gpg-sign -m "feat: initial release"
    $RootHistory = @(Get-ValidatedVersionHistory -RepoRoot $RootContractRepo -TargetSha HEAD)
    Assert-Equal 1 $RootHistory.Count "root contract history count"
    Assert-Equal "0.1.0" $RootHistory[0].version "root contract version"
    Assert-Equal "v0.1.0" $RootHistory[0].tag "root contract tag"

    Write-TestFile (Join-Path $SuccessRepo "package.json") (Set-ProductVersionInJson -Content $PreparedRoot -Version "0.3.0")
    & git -C $SuccessRepo add package.json
    & git -C $SuccessRepo -c user.name=uncord-Test -c user.email=test@example.invalid commit --quiet --no-gpg-sign -m "docs: invalid version bump"
    if ($LASTEXITCODE -ne 0) { throw "Failed to commit invalid history fixture." }
    $HistoryFailed = $false
    try { Get-ValidatedVersionHistory -RepoRoot $SuccessRepo -TargetSha HEAD | Out-Null } catch { $HistoryFailed = $true }
    if (-not $HistoryFailed) { throw "version history accepted an invalid transition" }
} finally {
    $ResolvedTemp = [IO.Path]::GetFullPath($TestRoot)
    if ($ResolvedTemp.StartsWith([IO.Path]::GetFullPath([IO.Path]::GetTempPath()), [StringComparison]::OrdinalIgnoreCase) -and (Test-Path -LiteralPath $ResolvedTemp)) {
        Remove-Item -LiteralPath $ResolvedTemp -Recurse -Force
    }
}

Write-Host "Versioning tests passed." -ForegroundColor Green
