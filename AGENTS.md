# uncord Agent Briefing

<project_context>
uncord is an open-source, low-latency multi-route Windows audio router built with a Rust audio core (WASAPI, lock-free SPSC ring buffers, adaptive clock-drift resampling), a headless CLI daemon, and a Tauri v2 + React 19 desktop studio. You are pair programming with the user to inspect, modify, test, benchmark, and release this codebase.
</project_context>

<engineering_principles>
1. Think before implementing: Inspect existing contracts (`crates/uncord-core/src/types.rs`, `backend.rs`, `engine.rs`, `src-tauri/src/ipc.rs`, `src/lib/ipc.ts`) and verify assumptions with repository tools first.
2. Surgical and minimal changes: Modify only the files and lines necessary for the assigned task. Preserve untouched code, existing formatting, and uncommitted user edits to keep review diffs clean.
3. Preserve real-time audio and memory budgets:
   - Audio hot-path zero-alloc rule: Real-time WASAPI capture and render threads must perform ZERO heap allocations, ZERO mutex locks, and ZERO blocking I/O. Communicate strictly through pre-allocated lock-free SPSC ring buffers (`ringbuf`) and atomic metrics (`RouteTelemetryAtomics`).
   - Software pipeline latency: Keep total software pipeline latency (capture period + ring buffer + render period) `<= 12.0 ms` in `UltraLow` mode.
   - Tray background RAM: Closing the main window to the system tray (or launching with `--start-minimized`) must destroy the WebView2 process via `window.destroy()` and stop UI telemetry emission so background RAM stays `<= 25 MB` (target `~6-15 MB`).
4. Preserve frozen IPC contracts: Keep Rust `#[serde(rename_all = "camelCase")]` field names and TypeScript definitions in `src/lib/ipc.ts` strictly aligned.
5. Clean your own artifacts: Remove unused imports, temporary files, and debug prints introduced by your changes.
</engineering_principles>

<repository_layout>
- crates/uncord-core/: Core Rust audio engine (WASAPI backend, channel mapper, PI-controlled cubic Hermite adaptive resampler, lock-free telemetry atomics, deterministic mock backend, atomic JSON store, and pipeline latency/drift benchmarks).
- crates/uncord-cli/: Headless CLI binary (`uncord-cli`) for device discovery (`--list-devices`) and background routing (`--headless`).
- src-tauri/: Tauri v2 Windows desktop shell, IPC command handlers (`UncordIpcContract`), system tray controller, and WebView2 lifecycle management.
- src/: React 19 + TypeScript + Tailwind v4 desktop studio UI with 60fps `<canvas>` Peak/RMS meters and profile/device controls.
- brand/: Hardware Port master SVG marks, exports, and `BRAND_GUIDELINES.md`.
- scripts/: Conventional Commit version synchronization, bilingual docs/asset gate verification, release backlog discovery, Windows artifact staging, and immutable GitHub Release verification scripts.
- .github/workflows/: Pinned-SHA CI validation and automated immutable Windows GitHub Release pipelines.
</repository_layout>

<versioning_and_git>
1. Single version authority: The root `package.json` `"version"` field governs all repository version declarations:
   - `package.json` (`"version": "..."`)
   - `Cargo.toml` (`[workspace.package]` `version = "..."`)
   - `src-tauri/tauri.conf.json` (`"version": "..."`)
   - `crates/uncord-core/src/lib.rs` (`pub const VERSION: &str = "...";`)
2. Synchronize before committing: Run the version preparation script with your planned commit message before creating a commit:
   powershell -File scripts/prepare-version.ps1 -Message "<commit message>"
   This script increments the version based on Conventional Commit semantics:
   - Major: Header containing '!' or body with 'BREAKING CHANGE:'
   - Minor: 'feat'
   - Patch: 'fix', 'perf', 'refactor', 'build', 'security'
   - No bump: 'docs', 'ci', 'test', 'chore' (and 'build(deps)' / 'chore(deps)')
3. Atomic commits: Use the identical Conventional Commit subject in both `prepare-version.ps1` and `git commit`. Stage only files relevant to the change, leaving uncommitted user work untouched.
</versioning_and_git>

<documentation_synchronization>
1. Mandatory per-commit check: Every change touching `crates/uncord-core/src/`, `crates/uncord-cli/src/`, `src-tauri/src/`, or `src/` must be accompanied by synchronized documentation updates (`README.md` + `README.tr.md` or `CHANGELOG.md`) before committing. Verify documentation state with:
   powershell -File scripts/verify-docs.ps1
2. Token-optimized surgical reading:
   - Read only the targeted line ranges (`StartLine`/`EndLine`) needed for an edit after locating headings with `Select-String` or `rg -n`.
   - Run `powershell -File scripts/verify-docs.ps1` to identify exact missing documentation targets before editing docs.
3. Source-to-documentation mapping matrix:
   - CLI (`crates/uncord-cli/src/**/*`): `README.md` + `README.tr.md` (CLI Usage & Flags) and `CHANGELOG.md`.
   - Core Engine & Audio Contracts (`crates/uncord-core/src/**/*`): `README.md` + `README.tr.md` (Architecture, Latency & Channel Modes) or `CHANGELOG.md`.
   - Desktop Shell & UI (`src-tauri/src/**/*`, `src/**/*`): `README.md` + `README.tr.md` (Desktop Studio / Tray behavior) or `CHANGELOG.md`.
   - Internal fix/refactor/perf: `CHANGELOG.md` entry under the relevant category.
4. Symmetrical bilingual lockstep (EN/TR):
   - `README.md` and `README.tr.md` must always be updated together in the identical commit.
5. Brand and icon asset integrity:
   - `brand/BRAND_GUIDELINES.md`, master lockup SVGs (`brand/uncord-lockup-color.svg`, `brand/uncord-lockup-dark.svg`), and required Windows Tauri icons (`src-tauri/icons/{32x32.png,128x128.png,128x128@2x.png,icon.png,icon.ico,tray-icon.png}`) must remain present and non-empty.
</documentation_synchronization>

<verification>
Run checks proportional to the change and inspect actual command output before declaring completion:

Rust engine, CLI & latency benchmark:
cargo test --workspace
cargo test -p uncord-core --release -- --nocapture bench_pipeline_latency_and_drift
cargo clippy --workspace -- -D warnings

Frontend & Tauri desktop shell:
pnpm typecheck
pnpm build
pnpm tauri build

Documentation & release readiness:
powershell -File scripts/verify-docs.ps1
powershell -File scripts/test-verify-docs.ps1
powershell -File scripts/test-versioning.ps1
powershell -File scripts/validate-version.ps1 -Message "<exact full commit message>"
powershell -File scripts/stage-release.ps1

Report check statuses as PASS, FAIL, or NOT EXECUTED based on actual execution. Resolve failures by fixing implementation behavior rather than weakening assertions or linters.
</verification>

<security_and_release>
1. Audio thread isolation: Never run blocking OS calls, logging I/O, or dynamic allocations inside WASAPI capture/render callbacks or SPSC processing loops.
2. Immutable releases: Every version-bumping commit on `main` produces a verified Windows portable bundle (`uncord-v<version>-windows-x64-portable.zip`), optional NSIS installer (`*.exe`), and SHA-256 checksum manifest (`uncord-v<version>-SHA256SUMS.txt`) verified by `scripts/test-existing-release.ps1` and published as an immutable GitHub Release.
3. Secret isolation: Signing keys and workflow tokens belong exclusively in encrypted GitHub Actions secrets. Never print, stage, or commit credentials.
</security_and_release>

<delegation_and_completion>
1. Bounded subagents: Assign focused, non-overlapping file ownership. Require subagents to report changed files, executed checks, and any unresolved assumptions.
2. Completion criteria: Mark a task complete only after requested behavior is verified, audio latency/memory invariants hold, relevant tests and checks pass, and the final git diff has been reviewed.
</delegation_and_completion>
