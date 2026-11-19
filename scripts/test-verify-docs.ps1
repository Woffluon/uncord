param([string]$RepoRoot)

Set-StrictMode -Version 3.0
$ErrorActionPreference = "Stop"

$ScriptPath = Join-Path $PSScriptRoot "verify-docs.ps1"
$TestRoot = Join-Path ([IO.Path]::GetTempPath()) ("uncord-docs-test-" + [Guid]::NewGuid().ToString("N"))

function Write-TestFile([string]$Path, [string]$Content) {
    $Parent = Split-Path -Parent $Path
    if ($Parent -and -not (Test-Path -LiteralPath $Parent)) {
        New-Item -ItemType Directory -Path $Parent -Force | Out-Null
    }
    [IO.File]::WriteAllText($Path, $Content, [Text.UTF8Encoding]::new($false))
}

function Invoke-VerifyDocs([string]$Repo, [string]$CommitSha) {
    $ArgsList = @("-NoProfile", "-ExecutionPolicy", "Bypass", "-File", $ScriptPath, "-RepoRoot", $Repo)
    if ($CommitSha) {
        $ArgsList += @("-CommitSha", $CommitSha)
    }
    $PrevEap = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    try {
        $Output = & powershell @ArgsList 2>&1
        $Code = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $PrevEap
    }
    return [pscustomobject]@{
        ExitCode = $Code
        Text = ($Output | ForEach-Object { $_.ToString() }) -join "`n"
    }
}

function Commit-Fixture([string]$Repo, [string]$Message) {
    & git -C $Repo add -A
    if ($LASTEXITCODE -ne 0) { throw "Failed to stage fixture changes in $Repo." }
    & git -C $Repo -c user.name=uncord-Test -c user.email=test@example.invalid commit --quiet --no-gpg-sign -m $Message
    if ($LASTEXITCODE -ne 0) { throw "Failed to commit fixture '$Message' in $Repo." }
    $Sha = (& git -C $Repo rev-parse HEAD).Trim()
    if ($LASTEXITCODE -ne 0) { throw "Failed to resolve HEAD in $Repo." }
    return $Sha
}

Write-Host "Running verify-docs.ps1 automated suite..." -ForegroundColor Cyan

try {
    $FixtureRepo = Join-Path $TestRoot "repo"
    New-Item -ItemType Directory -Path $FixtureRepo -Force | Out-Null
    & git -C $FixtureRepo init --quiet
    if ($LASTEXITCODE -ne 0) { throw "Failed to initialize temporary git repo." }
    & git -C $FixtureRepo config core.autocrlf false

    Write-TestFile (Join-Path $FixtureRepo "README.md") "# uncord`nEnglish docs.`n"
    Write-TestFile (Join-Path $FixtureRepo "README.tr.md") "# uncord`nTurkce dokumantasyon.`n"
    Write-TestFile (Join-Path $FixtureRepo "CHANGELOG.md") "# Changelog`n"
    Write-TestFile (Join-Path $FixtureRepo "assets\update-public-key.hex") "98477cb793eeaef02b61ca53e87a0094d51db4c6f910260b85fa46c54e497bea`n"
    Write-TestFile (Join-Path $FixtureRepo "brand\BRAND_GUIDELINES.md") "# uncord Brand Guidelines`n"
    Write-TestFile (Join-Path $FixtureRepo "brand\uncord-lockup-color.svg") "<svg viewBox='0 0 960 256'/>`n"
    Write-TestFile (Join-Path $FixtureRepo "brand\uncord-lockup-dark.svg") "<svg viewBox='0 0 960 256'/>`n"
    foreach ($IconName in @("32x32.png", "128x128.png", "128x128@2x.png", "icon.png", "icon.ico", "tray-icon.png")) {
        Write-TestFile (Join-Path $FixtureRepo "src-tauri\icons\$IconName") "PNG_ICON_FIXTURE`n"
    }
    Write-TestFile (Join-Path $FixtureRepo "crates\uncord-core\src\lib.rs") "pub const VERSION: &str = `"0.1.0`";`n"
    Write-TestFile (Join-Path $FixtureRepo "crates\uncord-core\src\engine.rs") "pub struct UncordEngine;`n"
    Write-TestFile (Join-Path $FixtureRepo "crates\uncord-cli\src\main.rs") "fn main() {}`n"
    Write-TestFile (Join-Path $FixtureRepo "src-tauri\src\lib.rs") "pub fn run() {}`n"
    Write-TestFile (Join-Path $FixtureRepo "src\App.tsx") "export function App() { return null; }`n"
    Commit-Fixture $FixtureRepo "chore: initial fixture" | Out-Null

    # Test 1: Asymmetric README (English updated without Turkish) -> exit 1
    Write-TestFile (Join-Path $FixtureRepo "README.md") "# uncord`nEnglish docs updated.`n"
    $AsymCommit = Commit-Fixture $FixtureRepo "docs: update english README only"
    $Res1 = Invoke-VerifyDocs -Repo $FixtureRepo -CommitSha $AsymCommit
    if ($Res1.ExitCode -ne 1 -or $Res1.Text -notmatch "Asymmetric root README") {
        throw "Test 1 Failed: Expected exit code 1 with Asymmetric root README error. Got exit $($Res1.ExitCode): $($Res1.Text)"
    }
    Write-Host "  PASS: Detected asymmetric README change (exit 1)" -ForegroundColor Green

    # Test 2: CLI source changed without docs or CHANGELOG -> exit 1
    Write-TestFile (Join-Path $FixtureRepo "crates\uncord-cli\src\main.rs") "fn main() { println!(`"flag`"); }`n"
    $CliCommit = Commit-Fixture $FixtureRepo "feat(cli): add headless flag"
    $Res2 = Invoke-VerifyDocs -Repo $FixtureRepo -CommitSha $CliCommit
    if ($Res2.ExitCode -ne 1 -or $Res2.Text -notmatch "CLI source code changed") {
        throw "Test 2 Failed: Expected exit code 1 with CLI source code changed error. Got exit $($Res2.ExitCode): $($Res2.Text)"
    }
    Write-Host "  PASS: Detected CLI documentation drift (exit 1)" -ForegroundColor Green

    # Test 3: Core source changed without docs or CHANGELOG -> exit 1
    Write-TestFile (Join-Path $FixtureRepo "crates\uncord-core\src\engine.rs") "pub struct UncordEngine; // updated`n"
    $CoreCommit = Commit-Fixture $FixtureRepo "fix(core): adjust adaptive resampler gain"
    $Res3 = Invoke-VerifyDocs -Repo $FixtureRepo -CommitSha $CoreCommit
    if ($Res3.ExitCode -ne 1 -or $Res3.Text -notmatch "Core audio engine source code changed") {
        throw "Test 3 Failed: Expected exit code 1 with Core audio engine source code changed error. Got exit $($Res3.ExitCode): $($Res3.Text)"
    }
    Write-Host "  PASS: Detected core audio engine documentation drift (exit 1)" -ForegroundColor Green

    # Test 4: Synchronized clean change (CLI + core + README.md + README.tr.md + CHANGELOG.md) -> exit 0
    Write-TestFile (Join-Path $FixtureRepo "crates\uncord-cli\src\main.rs") "fn main() { println!(`"v2`"); }`n"
    Write-TestFile (Join-Path $FixtureRepo "README.md") "# uncord`nUpdated English docs.`n"
    Write-TestFile (Join-Path $FixtureRepo "README.tr.md") "# uncord`nGuncellenmis Turkce dokumantasyon.`n"
    Write-TestFile (Join-Path $FixtureRepo "CHANGELOG.md") "# Changelog`n- Updated CLI flag`n"
    $SyncedCommit = Commit-Fixture $FixtureRepo "feat(cli): document new headless flag"
    $Res4 = Invoke-VerifyDocs -Repo $FixtureRepo -CommitSha $SyncedCommit
    if ($Res4.ExitCode -ne 0 -or $Res4.Text -notmatch "PASS") {
        throw "Test 4 Failed: Expected exit code 0 PASS on synchronized change. Got exit $($Res4.ExitCode): $($Res4.Text)"
    }
    Write-Host "  PASS: Accepted synchronized CLI + bilingual README + CHANGELOG commit (exit 0)" -ForegroundColor Green

    # Test 5: Clean working tree check -> exit 0
    $Res5 = Invoke-VerifyDocs -Repo $FixtureRepo -CommitSha ""
    if ($Res5.ExitCode -ne 0 -or $Res5.Text -notmatch "PASS") {
        throw "Test 5 Failed: Expected exit code 0 PASS on clean working tree. Got exit $($Res5.ExitCode): $($Res5.Text)"
    }
    Write-Host "  PASS: Clean working tree with valid brand/icon assets (exit 0)" -ForegroundColor Green

    # Test 6: Missing brand/icon asset -> exit 1
    Remove-Item -LiteralPath (Join-Path $FixtureRepo "src-tauri\icons\tray-icon.png") -Force
    $MissingAssetCommit = Commit-Fixture $FixtureRepo "chore: accidentally drop tray icon"
    $Res6 = Invoke-VerifyDocs -Repo $FixtureRepo -CommitSha $MissingAssetCommit
    if ($Res6.ExitCode -ne 1 -or $Res6.Text -notmatch "Missing required brand/icon asset") {
        throw "Test 6 Failed: Expected exit code 1 for missing brand/icon asset. Got exit $($Res6.ExitCode): $($Res6.Text)"
    }
    Write-Host "  PASS: Detected missing brand/icon asset (exit 1)" -ForegroundColor Green
} finally {
    $ResolvedTemp = [IO.Path]::GetFullPath($TestRoot)
    if ($ResolvedTemp.StartsWith([IO.Path]::GetFullPath([IO.Path]::GetTempPath()), [StringComparison]::OrdinalIgnoreCase) -and (Test-Path -LiteralPath $ResolvedTemp)) {
        Remove-Item -LiteralPath $ResolvedTemp -Recurse -Force
    }
}

Write-Host "`nAll verify-docs tests PASSED successfully." -ForegroundColor Green
