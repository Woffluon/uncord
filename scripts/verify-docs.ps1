param(
    [string]$CommitSha,
    [string]$Base,
    [string]$RepoRoot,
    [switch]$Strict
)

Set-StrictMode -Version 3.0
$ErrorActionPreference = "Stop"

$RepoRoot = if ($RepoRoot) { $RepoRoot } else { Join-Path $PSScriptRoot ".." }
$RepoRoot = (Resolve-Path $RepoRoot).Path

function Get-ChangedFiles {
    param([string]$RepoRoot, [string]$CommitSha, [string]$Base)

    if ($CommitSha) {
        $Files = @(& git -C $RepoRoot diff-tree --no-commit-id --name-only -r --root $CommitSha)
        if ($LASTEXITCODE -ne 0) { throw "Failed to inspect commit '$CommitSha'." }
        return @($Files | Where-Object { -not [string]::IsNullOrWhiteSpace($_) } | ForEach-Object { $_ -replace '\\', '/' })
    }
    if ($Base) {
        $Files = @(& git -C $RepoRoot diff --name-only "$Base..HEAD")
        if ($LASTEXITCODE -ne 0) { throw "Failed to inspect diff against base '$Base'." }
        return @($Files | Where-Object { -not [string]::IsNullOrWhiteSpace($_) } | ForEach-Object { $_ -replace '\\', '/' })
    }

    $PrevEap = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    try {
        & git -C $RepoRoot rev-parse --verify HEAD 2>$null | Out-Null
        $HasHead = ($LASTEXITCODE -eq 0)
    } finally {
        $ErrorActionPreference = $PrevEap
    }

    if ($HasHead) {
        $Working = @(& git -C $RepoRoot diff --name-only HEAD)
        if ($LASTEXITCODE -ne 0) { throw "Failed to inspect working tree diff against HEAD." }
        $Staged = @(& git -C $RepoRoot diff --cached --name-only)
        if ($LASTEXITCODE -ne 0) { throw "Failed to inspect staged diff." }
        $Untracked = @(& git -C $RepoRoot ls-files --others --exclude-standard)
        if ($LASTEXITCODE -ne 0) { throw "Failed to list untracked files." }
        $All = @($Working + $Staged + $Untracked)
    } else {
        $Staged = @(& git -C $RepoRoot diff --cached --name-only)
        if ($LASTEXITCODE -ne 0) { $Staged = @() }
        $Untracked = @(& git -C $RepoRoot ls-files --others --exclude-standard)
        if ($LASTEXITCODE -ne 0) { throw "Failed to list untracked files." }
        $All = @($Staged + $Untracked)
    }

    return @($All | Where-Object { -not [string]::IsNullOrWhiteSpace($_) } | ForEach-Object { $_ -replace '\\', '/' } | Select-Object -Unique)
}

function Test-RequiredAssets {
    param(
        [string]$RepoRoot,
        [string]$CommitSha,
        [System.Collections.Generic.List[string]]$Errors
    )

    $RequiredAssets = @(
        "brand/BRAND_GUIDELINES.md",
        "brand/uncord-lockup-color.svg",
        "brand/uncord-lockup-dark.svg",
        "src-tauri/icons/32x32.png",
        "src-tauri/icons/128x128.png",
        "src-tauri/icons/128x128@2x.png",
        "src-tauri/icons/icon.png",
        "src-tauri/icons/icon.ico",
        "src-tauri/icons/tray-icon.png"
    )

    if ($CommitSha) {
        $TreeLines = @(& git -C $RepoRoot ls-tree -r -l $CommitSha)
        if ($LASTEXITCODE -ne 0) { throw "Failed to inspect repository tree at commit '$CommitSha'." }
        $BlobSizes = @{}
        foreach ($Line in $TreeLines) {
            if ($Line -match '^\d+\s+blob\s+[0-9a-f]+\s+(?<size>\d+)\t(?<path>.+)$') {
                $BlobSizes[($Matches["path"] -replace '\\', '/')] = [int64]$Matches["size"]
            }
        }
        foreach ($Asset in $RequiredAssets) {
            if (-not $BlobSizes.ContainsKey($Asset) -or $BlobSizes[$Asset] -le 0) {
                $Errors.Add("Missing required brand/icon asset in commit ${CommitSha}: '$Asset'")
            }
        }
    } else {
        foreach ($Asset in $RequiredAssets) {
            $FullPath = Join-Path $RepoRoot ($Asset.Replace('/', [System.IO.Path]::DirectorySeparatorChar))
            if (-not (Test-Path -LiteralPath $FullPath)) {
                $Errors.Add("Missing required brand/icon asset: '$Asset'")
            } elseif ((Get-Item -LiteralPath $FullPath).Length -le 0) {
                $Errors.Add("Empty required brand/icon asset: '$Asset'")
            }
        }
    }
}

$Errors = [System.Collections.Generic.List[string]]::new()

Test-RequiredAssets -RepoRoot $RepoRoot -CommitSha $CommitSha -Errors $Errors

$Changed = @(Get-ChangedFiles -RepoRoot $RepoRoot -CommitSha $CommitSha -Base $Base)

if ($Changed.Count -eq 0) {
    if ($Errors.Count -gt 0) {
        Write-Host "[DOCS_VERIFY] FAIL: Asset integrity violations detected ($($Errors.Count) errors):" -ForegroundColor Red
        foreach ($Err in $Errors) {
            Write-Host "  * $Err" -ForegroundColor Red
        }
        exit 1
    }
    Write-Host "[DOCS_VERIFY] PASS: Working tree is clean and brand/icon assets are intact." -ForegroundColor Green
    exit 0
}

# Categorize changed source files
$CoreChanged = @($Changed | Where-Object { $_ -match '^crates/uncord-core/src/' })
$CliChanged = @($Changed | Where-Object { $_ -match '^crates/uncord-cli/src/' })
$TauriChanged = @($Changed | Where-Object { $_ -match '^src-tauri/src/' })
$UiChanged = @($Changed | Where-Object { $_ -match '^src/' })

# Documentation files in change
$ReadmeEn = $Changed -contains 'README.md'
$ReadmeTr = $Changed -contains 'README.tr.md'
$Changelog = $Changed -contains 'CHANGELOG.md'

# --- Bilingual README Symmetry Verification ---
if ($ReadmeEn -xor $ReadmeTr) {
    $Missing = if ($ReadmeEn) { "README.tr.md" } else { "README.md" }
    $Errors.Add("Asymmetric root README: README.md and README.tr.md must be updated in lockstep. Missing: $Missing")
}

# --- Source-to-Docs / CHANGELOG Synchronization ---
$HasSyncedDocsOrChangelog = ($ReadmeEn -and $ReadmeTr) -or $Changelog

if ($CliChanged.Count -gt 0 -and -not $HasSyncedDocsOrChangelog) {
    $Errors.Add("CLI source code changed ($($CliChanged -join ', ')), but neither README.md/README.tr.md nor CHANGELOG.md was updated.`n  Action required: Update CLI documentation in both README.md and README.tr.md, or add an entry to CHANGELOG.md.")
}

if ($CoreChanged.Count -gt 0 -and -not $HasSyncedDocsOrChangelog) {
    $Errors.Add("Core audio engine source code changed ($($CoreChanged -join ', ')), but neither README.md/README.tr.md nor CHANGELOG.md was updated.`n  Action required: Document behavior changes in both README.md and README.tr.md, or add an entry to CHANGELOG.md.")
}

if ($TauriChanged.Count -gt 0 -and -not $HasSyncedDocsOrChangelog) {
    $Errors.Add("Desktop shell source code changed ($($TauriChanged -join ', ')), but neither README.md/README.tr.md nor CHANGELOG.md was updated.`n  Action required: Document behavior changes in both README.md and README.tr.md, or add an entry to CHANGELOG.md.")
}

if ($UiChanged.Count -gt 0 -and -not $HasSyncedDocsOrChangelog) {
    $Errors.Add("Frontend UI source code changed ($($UiChanged -join ', ')), but neither README.md/README.tr.md nor CHANGELOG.md was updated.`n  Action required: Document UI changes in both README.md and README.tr.md, or add an entry to CHANGELOG.md.")
}

if ($Errors.Count -gt 0) {
    Write-Host "[DOCS_VERIFY] FAIL: Documentation sync violations detected ($($Errors.Count) errors):" -ForegroundColor Red
    foreach ($Err in $Errors) {
        Write-Host "  * $Err" -ForegroundColor Red
    }
    Write-Host "`nToken-optimized remediation guidelines:" -ForegroundColor Yellow
    Write-Host "  1. Use git diff or targeted search (Select-String / rg -n) to locate the exact section." -ForegroundColor Yellow
    Write-Host "  2. Update README.md and README.tr.md symmetrically in lockstep (or add a CHANGELOG.md entry for internal changes)." -ForegroundColor Yellow
    Write-Host "  3. Ensure brand/BRAND_GUIDELINES.md, brand/uncord-lockup-*.svg, and src-tauri/icons/* remain intact." -ForegroundColor Yellow
    exit 1
}

Write-Host "[DOCS_VERIFY] PASS: Documentation and brand/icon assets are in sync across all surfaces." -ForegroundColor Green
exit 0
