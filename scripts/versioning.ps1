Set-StrictMode -Version 3.0

function Get-ProductVersionFromJson {
    param([Parameter(Mandatory)][string]$Content)

    $Match = [regex]::Match(
        $Content,
        '(?m)^\s*"version"\s*:\s*"(?<version>\d+\.\d+\.\d+)"'
    )
    if (-not $Match.Success) {
        throw '"version" field was not found in package.json'
    }

    return $Match.Groups["version"].Value
}

function Set-ProductVersionInJson {
    param(
        [Parameter(Mandatory)][string]$Content,
        [Parameter(Mandatory)][string]$Version
    )

    $Pattern = '(?m)^(\s*"version"\s*:\s*")\d+\.\d+\.\d+(")'
    $Updated = [regex]::Replace($Content, $Pattern, "`${1}$Version`${2}", 1)
    if ($Updated -eq $Content -and (Get-ProductVersionFromJson $Content) -cne $Version) {
        throw "Failed to update version in package.json"
    }
    return $Updated
}

function Get-TauriVersionFromJson {
    param([Parameter(Mandatory)][string]$Content)

    $Match = [regex]::Match(
        $Content,
        '(?m)^\s*"version"\s*:\s*"(?<version>\d+\.\d+\.\d+)"'
    )
    if (-not $Match.Success) {
        throw '"version" field was not found in src-tauri/tauri.conf.json'
    }

    return $Match.Groups["version"].Value
}

function Set-TauriVersionInJson {
    param(
        [Parameter(Mandatory)][string]$Content,
        [Parameter(Mandatory)][string]$Version
    )

    $Pattern = '(?m)^(\s*"version"\s*:\s*")\d+\.\d+\.\d+(")'
    $Updated = [regex]::Replace($Content, $Pattern, "`${1}$Version`${2}", 1)
    if ($Updated -eq $Content -and (Get-TauriVersionFromJson $Content) -cne $Version) {
        throw "Failed to update version in src-tauri/tauri.conf.json"
    }
    return $Updated
}

function Get-CargoWorkspaceVersionFromToml {
    param([Parameter(Mandatory)][string]$Content)

    $Pattern = '(?ms)^\[workspace\.package\]\r?\n(?:(?!^\[)[^\r\n]*\r?\n)*?^\s*version\s*=\s*"(?<version>\d+\.\d+\.\d+)"'
    $Match = [regex]::Match($Content, $Pattern)
    if (-not $Match.Success) {
        throw '[workspace.package] version was not found in Cargo.toml'
    }

    return $Match.Groups["version"].Value
}

function Set-CargoWorkspaceVersionInToml {
    param(
        [Parameter(Mandatory)][string]$Content,
        [Parameter(Mandatory)][string]$Version
    )

    $Pattern = '(?ms)(?<prefix>^\[workspace\.package\]\r?\n(?:(?!^\[)[^\r\n]*\r?\n)*?^\s*version\s*=\s*")\d+\.\d+\.\d+(?<suffix>")'
    $Match = [regex]::Match($Content, $Pattern)
    if (-not $Match.Success) {
        throw '[workspace.package] version was not found in Cargo.toml'
    }

    $Evaluator = [System.Text.RegularExpressions.MatchEvaluator]{
        param($m)
        $m.Groups["prefix"].Value + $Version + $m.Groups["suffix"].Value
    }
    $Updated = [regex]::Replace($Content, $Pattern, $Evaluator, 1)
    if ($Updated -eq $Content -and (Get-CargoWorkspaceVersionFromToml $Content) -cne $Version) {
        throw "Failed to update [workspace.package] version in Cargo.toml"
    }
    return $Updated
}

function Get-CoreVersionFromRust {
    param([Parameter(Mandatory)][string]$Content)

    $Match = [regex]::Match(
        $Content,
        '(?m)^pub\s+const\s+VERSION\s*:\s*&str\s*=\s*"(?<version>\d+\.\d+\.\d+)"\s*;'
    )
    if (-not $Match.Success) {
        throw "pub const VERSION was not found in crates/uncord-core/src/lib.rs"
    }

    return $Match.Groups["version"].Value
}

function Set-CoreVersionInRust {
    param(
        [Parameter(Mandatory)][string]$Content,
        [Parameter(Mandatory)][string]$Version
    )

    $Pattern = '(?m)^(?<prefix>pub\s+const\s+VERSION\s*:\s*&str\s*=\s*")\d+\.\d+\.\d+(?<suffix>"\s*;)'
    $Match = [regex]::Match($Content, $Pattern)
    if (-not $Match.Success) {
        throw "pub const VERSION was not found in crates/uncord-core/src/lib.rs"
    }

    $Evaluator = [System.Text.RegularExpressions.MatchEvaluator]{
        param($m)
        $m.Groups["prefix"].Value + $Version + $m.Groups["suffix"].Value
    }
    $Updated = [regex]::Replace($Content, $Pattern, $Evaluator, 1)
    if ($Updated -eq $Content -and (Get-CoreVersionFromRust $Content) -cne $Version) {
        throw "Failed to update pub const VERSION in crates/uncord-core/src/lib.rs"
    }
    return $Updated
}

function Get-VersionBump {
    param([Parameter(Mandatory)][string]$Message)

    $Header = ($Message -split '\r?\n', 2)[0]
    $Match = [regex]::Match(
        $Header,
        '^(?<type>feat|fix|perf|refactor|build|security|docs|ci|test|chore)(?:\((?<scope>[a-z0-9][a-z0-9._/-]*)\))?(?<breaking>!)?: (?<description>\S.*)$'
    )
    if (-not $Match.Success) {
        throw "Invalid Conventional Commit message: '$Header'"
    }

    if ($Match.Groups["breaking"].Success -or $Message -match '(?m)^BREAKING CHANGE:\s+\S') {
        return "major"
    }

    $Type = $Match.Groups["type"].Value
    $Scope = if ($Match.Groups["scope"].Success) { $Match.Groups["scope"].Value } else { "" }

    if ($Scope -in "deps", "deps-dev" -and $Type -in "build", "chore") {
        return "none"
    }

    switch ($Type) {
        "feat" { return "minor" }
        { $_ -in "fix", "perf", "refactor", "build", "security" } { return "patch" }
        default { return "none" }
    }
}

function Get-NextProductVersion {
    param(
        [Parameter(Mandatory)][string]$Version,
        [Parameter(Mandatory)][ValidateSet("major", "minor", "patch", "none")][string]$Bump
    )

    if ($Version -notmatch '^(?<major>\d+)\.(?<minor>\d+)\.(?<patch>\d+)$') {
        throw "Unsupported product version: '$Version'"
    }

    $Major = [int]$Matches["major"]
    $Minor = [int]$Matches["minor"]
    $Patch = [int]$Matches["patch"]
    switch ($Bump) {
        "major" { return "$($Major + 1).0.0" }
        "minor" { return "$Major.$($Minor + 1).0" }
        "patch" { return "$Major.$Minor.$($Patch + 1)" }
        default { return $Version }
    }
}

function Assert-ReleaseBaseContract {
    param(
        [Parameter(Mandatory)][string]$Tag,
        [Parameter(Mandatory)][bool]$HasVersionContract
    )

    if (-not $HasVersionContract -and $Tag -cne "v0.1.0") {
        throw "Only the initial v0.1.0 release may predate the product-version contract."
    }
}

function Get-LatestReleaseSelection {
    param(
        [Parameter(Mandatory)][AllowEmptyCollection()][object[]]$Candidates,
        [Parameter(Mandatory)][string]$TargetSha
    )

    if ($Candidates.Count -eq 0) { return }
    $Latest = $Candidates[-1]
    return [pscustomobject]@{
        sha = $TargetSha
        version = $Latest.version
        tag = $Latest.tag
    }
}

function Get-ValidatedVersionHistory {
    param(
        [Parameter(Mandatory)][string]$RepoRoot,
        [Parameter(Mandatory)][string]$TargetSha
    )

    $Target = (& git -C $RepoRoot rev-parse "$TargetSha^{commit}" 2>$null).Trim()
    if ($LASTEXITCODE -ne 0) { throw "Cannot resolve version-history target '$TargetSha'." }

    $Commits = @(& git -C $RepoRoot rev-list --reverse --first-parent $Target)
    if ($LASTEXITCODE -ne 0) { throw "Cannot read first-parent history for '$Target'." }
    $ContractStart = $Commits | Where-Object {
        $Path = @(& git -C $RepoRoot ls-tree -r --name-only $_ -- scripts/versioning.ps1)
        $LASTEXITCODE -eq 0 -and $Path -contains "scripts/versioning.ps1"
    } | Select-Object -First 1
    if (-not $ContractStart) { return }

    $SeenVersions = @{}
    $ParentLine = (& git -C $RepoRoot rev-list --parents -n 1 $ContractStart).Trim()
    if ($LASTEXITCODE -ne 0) { throw "Cannot inspect parents for '$ContractStart'." }
    $ParentTokens = @($ParentLine -split '\s+' | Where-Object { $_ })
    $HasParent = ($ParentTokens.Count -gt 1)

    if ($HasParent) {
        $ContractCommits = @(& git -C $RepoRoot rev-list --reverse --first-parent "$ContractStart^..$Target")
        if ($LASTEXITCODE -ne 0) { throw "Cannot read version-contract history through '$Target'." }
    } else {
        $RootJson = (& git -C $RepoRoot show "${ContractStart}:package.json") -join "`n"
        if ($LASTEXITCODE -ne 0) { throw "Cannot read package.json at $ContractStart." }
        $RootVersion = Get-ProductVersionFromJson $RootJson
        $RootMessage = (& git -C $RepoRoot show -s --format=%B $ContractStart) -join "`n"
        if ($LASTEXITCODE -ne 0) { throw "Cannot read commit message at $ContractStart." }
        $RootBump = Get-VersionBump $RootMessage
        if ($RootBump -ne "none") {
            $SeenVersions[$RootVersion] = $true
        }
        [pscustomobject]@{
            sha = $ContractStart
            version = $RootVersion
            tag = "v$RootVersion"
            bump = $RootBump
        }
        $ContractCommits = @(& git -C $RepoRoot rev-list --reverse --first-parent "$ContractStart..$Target")
        if ($LASTEXITCODE -ne 0) { throw "Cannot read version-contract history through '$Target'." }
    }

    foreach ($Commit in $ContractCommits) {
        $Parent = (& git -C $RepoRoot rev-parse "$Commit^" 2>$null).Trim()
        if ($LASTEXITCODE -ne 0) { throw "Version-contract commit $Commit has no parent." }
        $CurrentJson = (& git -C $RepoRoot show "${Commit}:package.json") -join "`n"
        if ($LASTEXITCODE -ne 0) { throw "Cannot read package.json at $Commit." }
        $ParentJson = (& git -C $RepoRoot show "${Parent}:package.json") -join "`n"
        if ($LASTEXITCODE -ne 0) { throw "Cannot read package.json at parent $Parent." }
        $CurrentVersion = Get-ProductVersionFromJson $CurrentJson
        $ParentVersion = Get-ProductVersionFromJson $ParentJson
        $Message = (& git -C $RepoRoot show -s --format=%B $Commit) -join "`n"
        if ($LASTEXITCODE -ne 0) { throw "Cannot read commit message at $Commit." }
        $Bump = Get-VersionBump $Message
        $ExpectedVersion = Get-NextProductVersion -Version $ParentVersion -Bump $Bump
        if ($CurrentVersion -cne $ExpectedVersion) {
            throw "Commit $Commit requires product version $ExpectedVersion ($Bump), but contains $CurrentVersion."
        }
        if ($Bump -ne "none") {
            if ($SeenVersions.ContainsKey($CurrentVersion)) {
                throw "Product version is reused in version-contract history: $CurrentVersion"
            }
            $SeenVersions[$CurrentVersion] = $true
        }
        [pscustomobject]@{
            sha = $Commit
            version = $CurrentVersion
            tag = "v$CurrentVersion"
            bump = $Bump
        }
    }
}
