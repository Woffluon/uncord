use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use ringbuf::traits::{Producer, Split};
use ringbuf::HeapRb;

use crate::backend::{ActiveRouteHandle, AudioBackend, AudioError, RouteTelemetryAtomics};
use crate::dsp::{
    compute_ring_buffer_layout, estimated_transport_floor_ms, matches_source_device,
    AdaptiveResampler, AtomicChannelMode, AtomicGainControl, ChannelMapper, GainSmoother,
    LevelMeterAccumulator,
};
use crate::types::{
    AudioDeviceInfo, ChannelMode, DeviceChangeEvent, DeviceFlow, LatencyBreakdown, LatencyMode,
    RouteConfig, RouteStatus, RouteTelemetry, TransportKind,
};

type DeviceEventListener = Arc<dyn Fn(DeviceChangeEvent) + Send + Sync>;

struct MockState {
    devices: Mutex<Vec<AudioDeviceInfo>>,
    listeners: Mutex<Vec<DeviceEventListener>>,
    start_count: AtomicU64,
    simulated_drift_ppm: Mutex<f64>,
}

/// Handle allowing deterministic unit/integration tests to simulate hot-plug events and drift.
#[derive(Clone)]
pub struct MockBackendController {
    state: Arc<MockState>,
}

impl MockBackendController {
    pub fn set_devices(&self, devices: Vec<AudioDeviceInfo>) {
        if let Ok(mut guard) = self.state.devices.lock() {
            *guard = devices;
        }
    }

    pub fn add_device(&self, device: AudioDeviceInfo) {
        let id = device.id.clone();
        if let Ok(mut guard) = self.state.devices.lock() {
            guard.retain(|d| d.id != id);
            guard.push(device);
        }
        self.emit(DeviceChangeEvent::Added(id));
    }

    pub fn remove_device(&self, device_id: &str) {
        if let Ok(mut guard) = self.state.devices.lock() {
            guard.retain(|d| {
                d.id != device_id && d.id.strip_prefix("loopback::") != Some(device_id)
            });
        }
        self.emit(DeviceChangeEvent::Removed(device_id.to_string()));
    }

    pub fn set_simulated_drift_ppm(&self, ppm: f64) {
        if let Ok(mut guard) = self.state.simulated_drift_ppm.lock() {
            *guard = ppm;
        }
    }

    pub fn start_count(&self) -> u64 {
        self.state.start_count.load(Ordering::Relaxed)
    }

    pub fn emit(&self, event: DeviceChangeEvent) {
        let listeners = self
            .state
            .listeners
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default();
        for cb in listeners {
            cb(event.clone());
        }
    }
}

/// Deterministic virtual audio backend for unit tests, headless previews, and benchmarks.
pub struct MockAudioBackend {
    state: Arc<MockState>,
}

impl MockAudioBackend {
    pub fn new() -> Self {
        Self::new_with_controller().0
    }

    pub fn new_with_controller() -> (Self, MockBackendController) {
        let state = Arc::new(MockState {
            devices: Mutex::new(default_mock_devices()),
            listeners: Mutex::new(Vec::new()),
            start_count: AtomicU64::new(0),
            simulated_drift_ppm: Mutex::new(0.0),
        });
        (
            Self {
                state: Arc::clone(&state),
            },
            MockBackendController { state },
        )
    }
}

impl Default for MockAudioBackend {
    fn default() -> Self {
        Self::new()
    }
}

/// Default deterministic hardware device matrix matching typical guitar USB + Bluetooth + wired setup.
pub fn default_mock_devices() -> Vec<AudioDeviceInfo> {
    let blackstar = AudioDeviceInfo {
        id: "mock-blackstar-usb".to_string(),
        name: "Blackstar ID:Core V4 (USB Audio)".to_string(),
        flow: DeviceFlow::Capture,
        channels: 4,
        sample_rate: 48_000,
        min_period_frames: 96,
        default_period_frames: 480,
        supports_raw_mode: true,
        transport: TransportKind::Usb,
        is_default: true,
        is_bluetooth_hfp_warning: false,
    };

    let bt_a2dp = AudioDeviceInfo {
        id: "mock-bt-a2dp".to_string(),
        name: "WH-1000XM5 Stereo (Bluetooth)".to_string(),
        flow: DeviceFlow::Render,
        channels: 2,
        sample_rate: 44_100,
        min_period_frames: 128,
        default_period_frames: 441,
        supports_raw_mode: false,
        transport: TransportKind::BluetoothA2dp,
        is_default: false,
        is_bluetooth_hfp_warning: false,
    };

    let bt_hfp = AudioDeviceInfo {
        id: "mock-bt-hfp".to_string(),
        name: "WH-1000XM5 Hands-Free AG Audio".to_string(),
        flow: DeviceFlow::Render,
        channels: 1,
        sample_rate: 16_000,
        min_period_frames: 160,
        default_period_frames: 160,
        supports_raw_mode: false,
        transport: TransportKind::BluetoothHfp,
        is_default: false,
        is_bluetooth_hfp_warning: true,
    };

    let wired_monitors = AudioDeviceInfo {
        id: "mock-wired-monitors".to_string(),
        name: "Studio Monitors (Wired)".to_string(),
        flow: DeviceFlow::Render,
        channels: 2,
        sample_rate: 48_000,
        min_period_frames: 96,
        default_period_frames: 480,
        supports_raw_mode: true,
        transport: TransportKind::Wired,
        is_default: true,
        is_bluetooth_hfp_warning: false,
    };

    let loopback_wired = AudioDeviceInfo {
        id: "loopback::mock-wired-monitors".to_string(),
        name: "Studio Monitors (Wired)".to_string(),
        flow: DeviceFlow::Loopback,
        channels: 2,
        sample_rate: 48_000,
        min_period_frames: 96,
        default_period_frames: 480,
        supports_raw_mode: false,
        transport: TransportKind::Wired,
        is_default: false,
        is_bluetooth_hfp_warning: false,
    };

    let loopback_bt = AudioDeviceInfo {
        id: "loopback::mock-bt-a2dp".to_string(),
        name: "WH-1000XM5 Stereo (Bluetooth)".to_string(),
        flow: DeviceFlow::Loopback,
        channels: 2,
        sample_rate: 44_100,
        min_period_frames: 128,
        default_period_frames: 441,
        supports_raw_mode: false,
        transport: TransportKind::BluetoothA2dp,
        is_default: false,
        is_bluetooth_hfp_warning: false,
    };

    vec![
        blackstar,
        bt_a2dp,
        bt_hfp,
        wired_monitors,
        loopback_wired,
        loopback_bt,
    ]
}

impl AudioBackend for MockAudioBackend {
    fn list_devices(&self) -> Result<Vec<AudioDeviceInfo>, AudioError> {
        let guard = self
            .state
            .devices
            .lock()
            .map_err(|_| AudioError::Platform("mock device lock poisoned".into()))?;
        Ok(guard.clone())
    }

    fn start_route(&self, config: &RouteConfig) -> Result<Box<dyn ActiveRouteHandle>, AudioError> {
        let devices = self.list_devices()?;
        let is_loopback =
            config.source_is_loopback || config.source_device_id.starts_with("loopback::");

        let source = devices
            .iter()
            .find(|d| matches_source_device(d, &config.source_device_id, is_loopback))
            .cloned()
            .ok_or_else(|| AudioError::DeviceNotFound(config.source_device_id.clone()))?;

        let target = devices
            .iter()
            .find(|d| d.flow == DeviceFlow::Render && d.id == config.target_device_id)
            .cloned()
            .ok_or_else(|| AudioError::DeviceNotFound(config.target_device_id.clone()))?;

        if config.exclusive_output
            && matches!(
                target.transport,
                TransportKind::BluetoothA2dp | TransportKind::BluetoothHfp
            )
        {
            return Err(AudioError::ExclusiveModeRejected(
                target.id.clone(),
                "Bluetooth endpoints do not allow WASAPI exclusive mode".to_string(),
            ));
        }

        self.state.start_count.fetch_add(1, Ordering::Relaxed);
        let drift_ppm = self
            .state
            .simulated_drift_ppm
            .lock()
            .map(|g| *g)
            .unwrap_or(0.0);

        let cap_period_frames = match config.latency_mode {
            LatencyMode::UltraLow | LatencyMode::Low => source.min_period_frames.max(64),
            LatencyMode::Balanced | LatencyMode::Safe => source.default_period_frames.max(64),
        };
        let ren_period_frames = match config.latency_mode {
            LatencyMode::UltraLow | LatencyMode::Low => target.min_period_frames.max(64),
            LatencyMode::Balanced | LatencyMode::Safe => target.default_period_frames.max(64),
        };

        let layout = compute_ring_buffer_layout(
            source.sample_rate,
            cap_period_frames,
            target.sample_rate,
            ren_period_frames,
            config.latency_mode,
        );

        let atomics = Arc::new(RouteTelemetryAtomics::new());
        let cap_ms = (cap_period_frames as f32 / source.sample_rate as f32) * 1000.0;
        let ren_ms = (ren_period_frames as f32 / target.sample_rate as f32) * 1000.0;
        let floor_ms = estimated_transport_floor_ms(source.transport)
            + estimated_transport_floor_ms(target.transport);

        atomics.store_latency(LatencyBreakdown {
            capture_period_ms: cap_ms,
            ring_buffer_ms: layout.target_ms,
            render_period_ms: ren_ms,
            software_total_ms: cap_ms + layout.target_ms + ren_ms,
            estimated_transport_floor_ms: floor_ms,
            raw_mode_active: source.supports_raw_mode && !is_loopback,
            exclusive_active: config.exclusive_output,
        });

        let gain_ctrl = Arc::new(AtomicGainControl::new(config.gain_db, config.muted));
        let channel_ctrl = Arc::new(AtomicChannelMode::new(config.channel_mode));
        let stop_flag = Arc::new(AtomicBool::new(false));

        let atomics_worker = Arc::clone(&atomics);
        let gain_worker = Arc::clone(&gain_ctrl);
        let channel_worker = Arc::clone(&channel_ctrl);
        let stop_worker = Arc::clone(&stop_flag);
        let src_channels = source.channels.max(1) as usize;
        let src_sr = source.sample_rate;
        let dst_sr = target.sample_rate;
        let raw_mode = source.supports_raw_mode && !is_loopback;
        let exclusive = config.exclusive_output;

        let worker = thread::Builder::new()
            .name(format!("uncord-mock-{}", config.id))
            .spawn(move || {
                let rb = HeapRb::<f32>::new(layout.capacity_frames * 2);
                let (mut prod, mut cons) = rb.split();
                for _ in 0..(layout.target_frames * 2) {
                    let _ = prod.try_push(0.0);
                }

                let mut resampler = AdaptiveResampler::new(src_sr, dst_sr, layout);
                let mut smoother = GainSmoother::new(dst_sr, 0.0, false);
                let mut phase = 0.0_f64;
                let mut frame_scratch = [0.0_f32; 16];
                let step_ms = 5_u64;
                let nominal_cap_per_tick = (src_sr as f64 * (step_ms as f64 / 1000.0))
                    * (1.0 + drift_ppm * 1e-6);
                let ren_frames_per_tick = ((dst_sr as f64 * (step_ms as f64 / 1000.0)).round()
                    as usize)
                    .max(1);
                let mut cap_frac = 0.0_f64;

                while !stop_worker.load(Ordering::Acquire) {
                    cap_frac += nominal_cap_per_tick;
                    let cap_frames = cap_frac.floor() as usize;
                    cap_frac -= cap_frames as f64;
                    let mode = channel_worker.load();

                    for _ in 0..cap_frames {
                        let s0 = (phase * std::f64::consts::TAU * 220.0 / src_sr as f64).sin()
                            as f32
                            * 0.22;
                        let s1 = (phase * std::f64::consts::TAU * 330.0 / src_sr as f64).sin()
                            as f32
                            * 0.18;
                        let dry = (phase * std::f64::consts::TAU * 110.0 / src_sr as f64).sin()
                            as f32
                            * 0.15;
                        phase += 1.0;

                        let ch_len = src_channels.min(frame_scratch.len());
                        for (ch, slot) in frame_scratch.iter_mut().enumerate().take(ch_len) {
                            *slot = match ch {
                                0 => s0,
                                1 => s1,
                                _ => dry,
                            };
                        }
                        let (l, r) =
                            ChannelMapper::map_f32_frame(mode, &frame_scratch[..ch_len]);
                        if prod.push_slice(&[l, r]) < 2 {
                            atomics_worker.xruns.fetch_add(1, Ordering::Relaxed);
                        }
                    }

                    let (gain_db, muted) = gain_worker.load();
                    smoother.set_target(gain_db, muted);
                    let mut meter = LevelMeterAccumulator::new();
                    let report = resampler.process_stereo_block(
                        &mut cons,
                        ren_frames_per_tick,
                        |_, l, r| {
                            let g = smoother.next_gain();
                            let out_l = l * g;
                            let out_r = r * g;
                            meter.push_frame(out_l, out_r);
                        },
                    );
                    if report.underrun {
                        atomics_worker.xruns.fetch_add(1, Ordering::Relaxed);
                    }
                    let (pl, pr, rl, rr, clip) = meter.finish();
                    atomics_worker.store_levels(pl, pr, rl, rr, clip);
                    atomics_worker.store_buffer_metrics(report.fill_ratio, report.drift_ppm);
                    atomics_worker.store_latency(LatencyBreakdown {
                        capture_period_ms: cap_ms,
                        ring_buffer_ms: report.ring_buffer_ms,
                        render_period_ms: ren_ms,
                        software_total_ms: cap_ms + report.ring_buffer_ms + ren_ms,
                        estimated_transport_floor_ms: floor_ms,
                        raw_mode_active: raw_mode,
                        exclusive_active: exclusive,
                    });

                    thread::sleep(Duration::from_millis(step_ms));
                }
            })
            .map_err(|err| AudioError::BackendInit(format!("failed to spawn mock thread: {err}")))?;

        Ok(Box::new(MockRouteHandle {
            route_id: config.id.clone(),
            atomics,
            gain_ctrl,
            channel_ctrl,
            stop_flag,
            worker: Some(worker),
        }))
    }

    fn subscribe_device_changes(
        &self,
        callback: Box<dyn Fn(DeviceChangeEvent) + Send + Sync>,
    ) -> Result<(), AudioError> {
        let mut guard = self
            .state
            .listeners
            .lock()
            .map_err(|_| AudioError::Platform("mock listener lock poisoned".into()))?;
        guard.push(Arc::from(callback));
        Ok(())
    }
}

struct MockRouteHandle {
    route_id: String,
    atomics: Arc<RouteTelemetryAtomics>,
    gain_ctrl: Arc<AtomicGainControl>,
    channel_ctrl: Arc<AtomicChannelMode>,
    stop_flag: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl MockRouteHandle {
    fn stop_inner(&mut self) {
        self.stop_flag.store(true, Ordering::Release);
        if let Some(handle) = self.worker.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for MockRouteHandle {
    fn drop(&mut self) {
        self.stop_inner();
    }
}

impl ActiveRouteHandle for MockRouteHandle {
    fn route_id(&self) -> &str {
        &self.route_id
    }

    fn set_gain_db(&self, gain_db: f32) {
        self.gain_ctrl.set_gain_db(gain_db);
    }

    fn set_muted(&self, muted: bool) {
        self.gain_ctrl.set_muted(muted);
    }

    fn set_channel_mode(&self, mode: ChannelMode) {
        self.channel_ctrl.store(mode);
    }

    fn telemetry(&self) -> RouteTelemetry {
        self.atomics.snapshot(&self.route_id, RouteStatus::Running)
    }

    fn stop(mut self: Box<Self>) {
        self.stop_inner();
    }
}
