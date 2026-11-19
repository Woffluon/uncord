use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, Ordering};

use thiserror::Error;

use crate::types::{
    AudioDeviceInfo, ChannelMode, DeviceChangeEvent, LatencyBreakdown, RouteConfig, RouteStatus,
    RouteTelemetry,
};

/// Errors produced by audio enumeration, initialization, or routing backends.
#[derive(Debug, Error)]
pub enum AudioError {
    #[error("audio device not found: {0}")]
    DeviceNotFound(String),
    #[error("unsupported audio format on device {0}")]
    UnsupportedFormat(String),
    #[error("exclusive mode rejected on device {0}: {1}")]
    ExclusiveModeRejected(String, String),
    #[error("invalid route configuration: {0}")]
    InvalidConfig(String),
    #[error("backend initialization error: {0}")]
    BackendInit(String),
    #[error("platform audio API failure: {0}")]
    Platform(String),
}

/// Lock-free atomic metrics updated by capture/render threads and read by UI/CLI.
#[derive(Debug, Default)]
pub struct RouteTelemetryAtomics {
    pub peak_left_bits: AtomicU32,
    pub peak_right_bits: AtomicU32,
    pub rms_left_bits: AtomicU32,
    pub rms_right_bits: AtomicU32,
    pub clipping: AtomicBool,
    pub buffer_fill_bits: AtomicU32,
    pub drift_ppm_x100: AtomicI32,
    pub xruns: AtomicU64,
    pub capture_period_us: AtomicU32,
    pub ring_buffer_us: AtomicU32,
    pub render_period_us: AtomicU32,
    pub estimated_transport_floor_us: AtomicU32,
    pub raw_mode_active: AtomicBool,
    pub exclusive_active: AtomicBool,
}

impl RouteTelemetryAtomics {
    /// Creates zero-initialized telemetry atomics.
    pub fn new() -> Self {
        Self::default()
    }

    /// Updates instantaneous meter peaks, RMS values, and clipping flag from the render hot path.
    #[inline]
    pub fn store_levels(&self, peak_l: f32, peak_r: f32, rms_l: f32, rms_r: f32, clipping: bool) {
        self.peak_left_bits
            .store(peak_l.to_bits(), Ordering::Relaxed);
        self.peak_right_bits
            .store(peak_r.to_bits(), Ordering::Relaxed);
        self.rms_left_bits.store(rms_l.to_bits(), Ordering::Relaxed);
        self.rms_right_bits
            .store(rms_r.to_bits(), Ordering::Relaxed);
        if clipping {
            self.clipping.store(true, Ordering::Relaxed);
        }
    }

    /// Stores buffer fill ratio and adaptive clock-drift compensation in parts-per-million.
    #[inline]
    pub fn store_buffer_metrics(&self, fill_ratio: f32, drift_ppm: f32) {
        self.buffer_fill_bits
            .store(fill_ratio.to_bits(), Ordering::Relaxed);
        self.drift_ppm_x100
            .store((drift_ppm * 100.0) as i32, Ordering::Relaxed);
    }

    /// Records measured pipeline period durations and active hardware mode flags.
    pub fn store_latency(&self, breakdown: LatencyBreakdown) {
        self.capture_period_us
            .store((breakdown.capture_period_ms * 1000.0) as u32, Ordering::Relaxed);
        self.ring_buffer_us
            .store((breakdown.ring_buffer_ms * 1000.0) as u32, Ordering::Relaxed);
        self.render_period_us
            .store((breakdown.render_period_ms * 1000.0) as u32, Ordering::Relaxed);
        self.estimated_transport_floor_us.store(
            (breakdown.estimated_transport_floor_ms * 1000.0) as u32,
            Ordering::Relaxed,
        );
        self.raw_mode_active
            .store(breakdown.raw_mode_active, Ordering::Relaxed);
        self.exclusive_active
            .store(breakdown.exclusive_active, Ordering::Relaxed);
    }

    /// Captures a coherent snapshot of current route telemetry.
    pub fn snapshot(&self, route_id: &str, status: RouteStatus) -> RouteTelemetry {
        let capture_ms = self.capture_period_us.load(Ordering::Relaxed) as f32 / 1000.0;
        let ring_ms = self.ring_buffer_us.load(Ordering::Relaxed) as f32 / 1000.0;
        let render_ms = self.render_period_us.load(Ordering::Relaxed) as f32 / 1000.0;
        let floor_ms = self.estimated_transport_floor_us.load(Ordering::Relaxed) as f32 / 1000.0;

        RouteTelemetry {
            route_id: route_id.to_string(),
            status,
            peak_left: f32::from_bits(self.peak_left_bits.load(Ordering::Relaxed)),
            peak_right: f32::from_bits(self.peak_right_bits.load(Ordering::Relaxed)),
            rms_left: f32::from_bits(self.rms_left_bits.load(Ordering::Relaxed)),
            rms_right: f32::from_bits(self.rms_right_bits.load(Ordering::Relaxed)),
            clipping: self.clipping.swap(false, Ordering::Relaxed),
            buffer_fill_ratio: f32::from_bits(self.buffer_fill_bits.load(Ordering::Relaxed)),
            drift_ppm: self.drift_ppm_x100.load(Ordering::Relaxed) as f32 / 100.0,
            xruns: self.xruns.load(Ordering::Relaxed),
            latency: LatencyBreakdown {
                capture_period_ms: capture_ms,
                ring_buffer_ms: ring_ms,
                render_period_ms: render_ms,
                software_total_ms: capture_ms + ring_ms + render_ms,
                estimated_transport_floor_ms: floor_ms,
                raw_mode_active: self.raw_mode_active.load(Ordering::Relaxed),
                exclusive_active: self.exclusive_active.load(Ordering::Relaxed),
            },
        }
    }
}

/// Live control handle for an active capture-to-render audio route.
pub trait ActiveRouteHandle: Send + Sync {
    fn route_id(&self) -> &str;
    fn set_gain_db(&self, gain_db: f32);
    fn set_muted(&self, muted: bool);
    fn set_channel_mode(&self, mode: ChannelMode);
    fn telemetry(&self) -> RouteTelemetry;
    fn stop(self: Box<Self>);
}

/// Platform abstraction for low-latency audio device enumeration and routing.
pub trait AudioBackend: Send + Sync {
    fn list_devices(&self) -> Result<Vec<AudioDeviceInfo>, AudioError>;
    fn start_route(&self, config: &RouteConfig) -> Result<Box<dyn ActiveRouteHandle>, AudioError>;
    fn subscribe_device_changes(
        &self,
        callback: Box<dyn Fn(DeviceChangeEvent) + Send + Sync>,
    ) -> Result<(), AudioError>;
}
