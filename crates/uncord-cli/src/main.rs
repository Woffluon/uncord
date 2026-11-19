use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use clap::{Parser, ValueEnum};
use uncord_core::backend::AudioError;
use uncord_core::engine::UncordEngine;
use uncord_core::mock::MockAudioBackend;
use uncord_core::types::{
    AudioDeviceInfo, ChannelMode, DeviceFlow, LatencyMode, RouteConfig, RouteStatus, TransportKind,
};

static KEEP_RUNNING: AtomicBool = AtomicBool::new(true);

#[cfg(windows)]
mod ctrl_c {
    use super::KEEP_RUNNING;
    use std::sync::atomic::Ordering;

    #[link(name = "kernel32")]
    extern "system" {
        fn SetConsoleCtrlHandler(
            handler_routine: Option<unsafe extern "system" fn(u32) -> i32>,
            add: i32,
        ) -> i32;
    }

    unsafe extern "system" fn handler(_ctrl_type: u32) -> i32 {
        KEEP_RUNNING.store(false, Ordering::Release);
        1
    }

    pub fn install() {
        // SAFETY: Registers a lock-free signal handler setting an atomic boolean.
        unsafe {
            let _ = SetConsoleCtrlHandler(Some(handler), 1);
        }
    }
}

#[cfg(not(windows))]
mod ctrl_c {
    pub fn install() {}
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CliLatencyMode {
    UltraLow,
    Low,
    Balanced,
    Safe,
}

impl From<CliLatencyMode> for LatencyMode {
    fn from(value: CliLatencyMode) -> Self {
        match value {
            CliLatencyMode::UltraLow => LatencyMode::UltraLow,
            CliLatencyMode::Low => LatencyMode::Low,
            CliLatencyMode::Balanced => LatencyMode::Balanced,
            CliLatencyMode::Safe => LatencyMode::Safe,
        }
    }
}

#[derive(Debug, Parser)]
#[command(
    name = "uncord-cli",
    version = uncord_core::VERSION,
    about = "Zero-UI headless CLI for the uncord ultra-low-latency audio router"
)]
struct Cli {
    /// List available audio capture, loopback, and render endpoints.
    #[arg(long)]
    list_devices: bool,

    /// Emit JSON output (used with --list-devices).
    #[arg(long)]
    json: bool,

    /// Run headless audio routing until Ctrl-C.
    #[arg(long)]
    headless: bool,

    /// Saved profile name or ID to load in --headless mode.
    #[arg(long, value_name = "NAME")]
    profile: Option<String>,

    /// Source device ID or substring match (capture, or render when --loopback is set).
    #[arg(long, value_name = "DEVICE")]
    source: Option<String>,

    /// Target render device ID or substring match.
    #[arg(long, value_name = "DEVICE")]
    target: Option<String>,

    /// Channel extraction: e.g. 1-2, 3-4, mono:4, or all.
    #[arg(long, default_value = "1-2", value_name = "SPEC")]
    channels: String,

    /// Route gain in decibels (-60.0 to +12.0).
    #[arg(long, default_value_t = 0.0, allow_hyphen_values = true, value_name = "DB")]
    gain_db: f32,

    /// Capture system loopback output of the specified --source render endpoint.
    #[arg(long)]
    loopback: bool,

    /// Request WASAPI exclusive mode on the target render endpoint.
    #[arg(long)]
    exclusive: bool,

    /// Pipeline latency mode.
    #[arg(long, value_enum, default_value_t = CliLatencyMode::UltraLow)]
    latency: CliLatencyMode,

    /// Override config directory (defaults to %APPDATA%\uncord).
    #[arg(long, hide = true, value_name = "DIR")]
    config_dir: Option<PathBuf>,

    /// Use deterministic MockAudioBackend instead of WASAPI (for testing).
    #[arg(long, hide = true)]
    mock: bool,
}

fn parse_channel_mode(spec: &str) -> Result<ChannelMode, String> {
    let trimmed = spec.trim();
    if trimmed.eq_ignore_ascii_case("all") {
        return Ok(ChannelMode::MixAllToStereo);
    }

    if let Some(rest) = trimmed
        .strip_prefix("mono:")
        .or_else(|| trimmed.strip_prefix("MONO:"))
    {
        let ch: u16 = rest
            .trim()
            .parse()
            .map_err(|_| format!("invalid mono channel '{spec}', expected e.g. mono:4"))?;
        if ch == 0 {
            return Err("channel numbers are 1-based (expected >= 1, e.g. mono:1)".to_string());
        }
        return Ok(ChannelMode::MonoToStereo { channel: ch - 1 });
    }

    if let Some((l_str, r_str)) = trimmed.split_once('-') {
        let left: u16 = l_str
            .trim()
            .parse()
            .map_err(|_| format!("invalid channel pair '{spec}', expected e.g. 1-2 or 3-4"))?;
        let right: u16 = r_str
            .trim()
            .parse()
            .map_err(|_| format!("invalid channel pair '{spec}', expected e.g. 1-2 or 3-4"))?;
        if left == 0 || right == 0 {
            return Err("channel numbers are 1-based (expected >= 1, e.g. 1-2)".to_string());
        }
        return Ok(ChannelMode::StereoPair {
            left: left - 1,
            right: right - 1,
        });
    }

    Err(format!(
        "unsupported channel spec '{spec}': expected 1-2, 3-4, mono:4, or all"
    ))
}

fn find_device<'a>(
    devices: &'a [AudioDeviceInfo],
    query: &str,
    desired_flow: DeviceFlow,
) -> Result<&'a AudioDeviceInfo, String> {
    let q_lower = query.trim().to_ascii_lowercase();
    if q_lower.is_empty() {
        return Err("device selector cannot be empty".to_string());
    }

    let pool: Vec<&AudioDeviceInfo> = devices
        .iter()
        .filter(|d| d.flow == desired_flow)
        .collect();

    if let Some(exact) = pool.iter().find(|d| d.id == query) {
        return Ok(*exact);
    }

    let matches: Vec<&AudioDeviceInfo> = pool
        .into_iter()
        .filter(|d| {
            d.id.to_ascii_lowercase().contains(&q_lower)
                || d.name.to_ascii_lowercase().contains(&q_lower)
        })
        .collect();

    match matches.len() {
        0 => Err(format!(
            "no {:?} device matching '{query}' (run --list-devices to inspect available endpoints)",
            desired_flow
        )),
        1 => Ok(matches[0]),
        _ => {
            if let Some(def) = matches.iter().find(|d| d.is_default) {
                return Ok(*def);
            }
            let names: Vec<String> = matches
                .iter()
                .map(|d| format!("{} [{}]", d.name, d.id))
                .collect();
            Err(format!(
                "ambiguous {:?} selector '{query}' matched multiple devices: {}",
                desired_flow,
                names.join(", ")
            ))
        }
    }
}

fn create_engine(cli: &Cli) -> Result<UncordEngine, AudioError> {
    if cli.mock {
        let dir = cli
            .config_dir
            .clone()
            .unwrap_or_else(|| std::env::temp_dir().join("uncord-cli-mock"));
        UncordEngine::new_with_backend(Box::new(MockAudioBackend::new()), dir)
    } else if let Some(dir) = &cli.config_dir {
        #[cfg(windows)]
        {
            let backend = Box::new(uncord_core::wasapi::WasapiBackend::new()?);
            UncordEngine::new_with_backend(backend, dir.clone())
        }
        #[cfg(not(windows))]
        {
            let _ = dir;
            UncordEngine::new_default()
        }
    } else {
        UncordEngine::new_default()
    }
}

fn format_transport(t: TransportKind, hfp_warning: bool) -> &'static str {
    match t {
        TransportKind::Usb => "USB",
        TransportKind::BluetoothA2dp => "BT-A2DP",
        TransportKind::BluetoothHfp => {
            if hfp_warning {
                "BT-HFP [WARN: mono/low-quality]"
            } else {
                "BT-HFP"
            }
        }
        TransportKind::Wireless24Ghz => "2.4GHz",
        TransportKind::Wired => "Wired",
        TransportKind::VirtualCable => "Virtual",
        TransportKind::Unknown => "Unknown",
    }
}

fn print_devices_table(devices: &[AudioDeviceInfo]) {
    if devices.is_empty() {
        println!("No active audio devices found.");
        return;
    }

    for dev in devices {
        let flow = match dev.flow {
            DeviceFlow::Capture => "CAPTURE ",
            DeviceFlow::Loopback => "LOOPBACK",
            DeviceFlow::Render => "RENDER  ",
        };
        let def = if dev.is_default { "*" } else { " " };
        let min_ms = (dev.min_period_frames as f32 / dev.sample_rate.max(1) as f32) * 1000.0;
        let raw = if dev.supports_raw_mode { "RAW" } else { "-" };
        println!(
            "{def} [{flow}] {name} | {ch}ch @ {sr} Hz | min {min_ms:.2} ms | {tr} | raw:{raw} | id={id}",
            name = dev.name,
            ch = dev.channels,
            sr = dev.sample_rate,
            tr = format_transport(dev.transport, dev.is_bluetooth_hfp_warning),
            id = dev.id,
        );
    }
}

fn run(cli: Cli) -> Result<(), String> {
    let engine = create_engine(&cli).map_err(|e| format!("failed to initialize engine: {e}"))?;

    if cli.list_devices {
        let devices = engine
            .list_devices()
            .map_err(|e| format!("failed to enumerate devices: {e}"))?;
        if cli.json {
            let json = serde_json::to_string_pretty(&devices)
                .map_err(|e| format!("JSON serialization error: {e}"))?;
            println!("{json}");
        } else {
            print_devices_table(&devices);
        }
        engine.shutdown();
        return Ok(());
    }

    if !cli.headless {
        engine.shutdown();
        return Err(
            "specify --list-devices or --headless (run --help for available flags)".to_string(),
        );
    }

    ctrl_c::install();
    KEEP_RUNNING.store(true, Ordering::Release);

    if let Some(profile_name) = &cli.profile {
        let snap = engine
            .load_profile(profile_name)
            .map_err(|e| format!("failed to load profile '{profile_name}': {e}"))?;
        println!(
            "uncord headless active: loaded profile '{profile_name}' ({} routes). Press Ctrl-C to stop.",
            snap.routes.len()
        );
    } else {
        let source_q = cli
            .source
            .as_deref()
            .ok_or("--headless requires either --profile <name> or --source <device> --target <device>")?;
        let target_q = cli
            .target
            .as_deref()
            .ok_or("--headless requires --target <device> when --source is specified")?;

        let channel_mode = parse_channel_mode(&cli.channels)?;
        let devices = engine
            .list_devices()
            .map_err(|e| format!("failed to list devices: {e}"))?;

        let is_loopback = cli.loopback || source_q.starts_with("loopback::");
        let src_flow = if is_loopback {
            DeviceFlow::Loopback
        } else {
            DeviceFlow::Capture
        };

        let src_dev = find_device(&devices, source_q, src_flow)?;
        let dst_dev = find_device(&devices, target_q, DeviceFlow::Render)?;

        if dst_dev.is_bluetooth_hfp_warning {
            eprintln!(
                "WARNING: '{}' uses Bluetooth Hands-Free (HFP) mono/narrowband profile; prefer Stereo A2DP for full-range audio.",
                dst_dev.name
            );
        }

        let route = RouteConfig {
            id: uuid_or_cli_id(),
            name: format!("{} -> {}", src_dev.name, dst_dev.name),
            source_device_id: src_dev.id.clone(),
            source_is_loopback: is_loopback,
            channel_mode,
            target_device_id: dst_dev.id.clone(),
            gain_db: cli.gain_db,
            muted: false,
            enabled: true,
            exclusive_output: cli.exclusive,
            latency_mode: cli.latency.into(),
        };

        let active = engine
            .add_route(route)
            .map_err(|e| format!("failed to start route: {e}"))?;

        println!(
            "uncord headless routing active: '{}' (latency={:?}, exclusive={}, gain={:.1} dB). Press Ctrl-C to stop.",
            active.name, active.latency_mode, active.exclusive_output, active.gain_db
        );
    }

    let mut ticks = 0_u64;
    while KEEP_RUNNING.load(Ordering::Acquire) {
        thread::sleep(Duration::from_millis(200));
        ticks += 1;
        if ticks.is_multiple_of(10) {
            for t in engine.get_telemetry() {
                if let RouteStatus::Error { ref message } = t.status {
                    eprintln!("route '{}' error: {message}", t.route_id);
                } else {
                    println!(
                        "[{}] status={:?} sw={:.2}ms (cap={:.2} ring={:.2} ren={:.2}) floor={:.1}ms drift={:+.1}ppm xruns={} peak=({:.3},{:.3})",
                        t.route_id,
                        t.status,
                        t.latency.software_total_ms,
                        t.latency.capture_period_ms,
                        t.latency.ring_buffer_ms,
                        t.latency.render_period_ms,
                        t.latency.estimated_transport_floor_ms,
                        t.drift_ppm,
                        t.xruns,
                        t.peak_left,
                        t.peak_right,
                    );
                }
            }
        }
    }

    println!("Shutting down uncord headless audio engine...");
    engine.shutdown();
    Ok(())
}

fn uuid_or_cli_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("cli-{nanos}")
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uncord_core::mock::default_mock_devices;

    #[test]
    fn channel_spec_parses_1based_pairs_mono_and_all() {
        assert_eq!(
            parse_channel_mode("1-2").unwrap(),
            ChannelMode::StereoPair { left: 0, right: 1 }
        );
        assert_eq!(
            parse_channel_mode("3-4").unwrap(),
            ChannelMode::StereoPair { left: 2, right: 3 }
        );
        assert_eq!(
            parse_channel_mode("mono:4").unwrap(),
            ChannelMode::MonoToStereo { channel: 3 }
        );
        assert_eq!(
            parse_channel_mode("all").unwrap(),
            ChannelMode::MixAllToStereo
        );
        assert!(parse_channel_mode("0-1").is_err());
        assert!(parse_channel_mode("mono:0").is_err());
        assert!(parse_channel_mode("bogus").is_err());
    }

    #[test]
    fn device_lookup_matches_substring_and_loopback() {
        let devices = default_mock_devices();
        let src = find_device(&devices, "blackstar", DeviceFlow::Capture).unwrap();
        assert_eq!(src.id, "mock-blackstar-usb");

        let dst = find_device(&devices, "wh-1000xm5 stereo", DeviceFlow::Render).unwrap();
        assert_eq!(dst.id, "mock-bt-a2dp");

        let lb = find_device(&devices, "studio monitors", DeviceFlow::Loopback).unwrap();
        assert_eq!(lb.id, "loopback::mock-wired-monitors");
    }

    #[test]
    fn clap_parses_negative_gain_and_headless_flags() {
        let cli = Cli::try_parse_from([
            "uncord-cli",
            "--headless",
            "--source",
            "Blackstar",
            "--target",
            "WH-1000XM5",
            "--channels",
            "3-4",
            "--gain-db",
            "-6.5",
            "--loopback",
            "--exclusive",
            "--latency",
            "ultra-low",
        ])
        .expect("clap should parse negative gain-db cleanly");

        assert!(cli.headless);
        assert_eq!(cli.gain_db, -6.5);
        assert_eq!(cli.channels, "3-4");
        assert!(cli.loopback);
        assert!(cli.exclusive);
    }
}
