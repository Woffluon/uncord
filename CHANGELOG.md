# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [0.1.0] - 2026-11-19

### Added
- **WASAPI Low-Latency Audio Engine (`crates/uncord-core`)**:
  - Direct WASAPI capture-to-render and loopback routing (`windows` v0.61) with `COINIT_MULTITHREADED` COM initialization, `"Pro Audio"` MMCSS thread priority, `IAudioClient2` raw APO bypass (`AUDCLNT_STREAMOPTIONS_RAW`), `IAudioClient3` shared minimum-period initialization, and optional `AUDCLNT_SHAREMODE_EXCLUSIVE` render output.
  - Lock-free SPSC ring buffer (`ringbuf::HeapRb<f32>`) pipeline with zero heap allocations, zero mutexes, and zero I/O inside capture and render hot paths (`<= 12.0 ms` software pipeline latency).
  - PI-controlled fractional cubic Hermite adaptive clock-drift resampler (`AdaptiveResampler`, `±2000 ppm`) that keeps independent USB and wireless audio device clocks synchronized without buffer overflows or dropouts.
  - Multi-channel extraction (`ChannelMapper`) supporting `StereoPair` (e.g., Channels 1–2 processed tone and Channels 3–4 reamp/DI on 4-channel USB-C amplifiers such as Blackstar ID:Core V4), `MonoToStereo`, and `MixAllToStereo`.
  - Bluetooth Hands-Free (`BluetoothHfp`, 8–16 kHz mono SCO) detection with explicit `isBluetoothHfpWarning` telemetry and physical transport latency floor reporting alongside software pipeline metrics (`LatencyBreakdown`).
  - Persistent atomic JSON configuration and routing profiles (`store.rs`) plus `IMMNotificationClient` hot-plug auto-reconnect.
- **Headless CLI (`crates/uncord-cli`)**:
  - Added `--version`, `--list-devices [--json]`, `--headless --profile <name>`, and direct CLI route flags (`--source`, `--target`, `--channels`, `--gain-db`, `--loopback`, `--exclusive`, `--latency`).
- **Tauri v2 Desktop Studio & System Tray (`src-tauri`, `src`)**:
  - Added frozen IPC contract (`UncordIpcContract` / `uncordIpc`) with `~30 Hz` live telemetry (`uncord://telemetry`), hot-plug device change notifications (`uncord://devices-changed`), and route status updates.
  - Implemented WebView2 process destruction (`window.destroy()`) on close-to-tray and `--start-minimized` startup so background tray memory remains `<= 25 MB`.
  - Built React 19 + Tailwind v4 studio UI in Obsidian Dark and Studio Chalk themes with 60fps `<canvas>` Peak/RMS VU meters, per-route gain/mute/channel/latency controls, Bluetooth HFP inline warnings, and profile management.
- **Brand Identity, Verification & Immutable Release Pipeline**:
  - Added Hardware Port brand identity assets (`brand/`), Windows Tauri/web icons (`src-tauri/icons/`, `public/icons/`), bilingual documentation (`README.md` / `README.tr.md`), version sync & doc gate scripts (`scripts/`), and pinned-SHA GitHub Actions CI & immutable release workflows (`.github/workflows/`).
