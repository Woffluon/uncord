use serde::{Deserialize, Serialize};

/// Data-flow direction of an audio endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DeviceFlow {
    Capture,
    Render,
    Loopback,
}

/// Physical or virtual transport classification for latency diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TransportKind {
    Usb,
    BluetoothA2dp,
    BluetoothHfp,
    Wireless24Ghz,
    Wired,
    VirtualCable,
    Unknown,
}

/// Discovered audio endpoint metadata exposed to CLI and UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioDeviceInfo {
    pub id: String,
    pub name: String,
    pub flow: DeviceFlow,
    pub channels: u16,
    pub sample_rate: u32,
    pub min_period_frames: u32,
    pub default_period_frames: u32,
    pub supports_raw_mode: bool,
    pub transport: TransportKind,
    pub is_default: bool,
    pub is_bluetooth_hfp_warning: bool,
}

/// Channel extraction strategy for multi-channel capture sources (e.g., 4-channel USB amps).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ChannelMode {
    StereoPair { left: u16, right: u16 },
    MonoToStereo { channel: u16 },
    MixAllToStereo,
}

impl Default for ChannelMode {
    fn default() -> Self {
        Self::StereoPair { left: 0, right: 1 }
    }
}

/// Buffer and period target preset controlling software pipeline delay vs jitter resilience.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum LatencyMode {
    #[default]
    UltraLow,
    Low,
    Balanced,
    Safe,
}

/// Persistent configuration for a single source-to-target audio route.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteConfig {
    pub id: String,
    pub name: String,
    pub source_device_id: String,
    pub source_is_loopback: bool,
    pub channel_mode: ChannelMode,
    pub target_device_id: String,
    pub gain_db: f32,
    pub muted: bool,
    pub enabled: bool,
    pub exclusive_output: bool,
    pub latency_mode: LatencyMode,
}

/// Runtime lifecycle state of an audio route.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum RouteStatus {
    Stopped,
    Running,
    Reconnecting,
    Error { message: String },
}

/// Measured software pipeline latency vs physical transport baseline.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LatencyBreakdown {
    pub capture_period_ms: f32,
    pub ring_buffer_ms: f32,
    pub render_period_ms: f32,
    pub software_total_ms: f32,
    pub estimated_transport_floor_ms: f32,
    pub raw_mode_active: bool,
    pub exclusive_active: bool,
}

/// Real-time telemetry snapshot emitted for a route without blocking audio threads.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteTelemetry {
    pub route_id: String,
    pub status: RouteStatus,
    pub peak_left: f32,
    pub peak_right: f32,
    pub rms_left: f32,
    pub rms_right: f32,
    pub clipping: bool,
    pub buffer_fill_ratio: f32,
    pub drift_ppm: f32,
    pub xruns: u64,
    pub latency: LatencyBreakdown,
}

/// Saved routing preset.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub routes: Vec<RouteConfig>,
}

fn default_true() -> bool {
    true
}

/// Persistent application settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub minimize_to_tray_on_close: bool,
    pub start_minimized: bool,
    pub auto_reconnect: bool,
    #[serde(default = "default_true")]
    pub update_checks_enabled: bool,
    pub active_profile_id: Option<String>,
    pub theme: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            minimize_to_tray_on_close: true,
            start_minimized: false,
            auto_reconnect: true,
            update_checks_enabled: true,
            active_profile_id: None,
            theme: "dark".to_string(),
        }
    }
}

/// Full engine state returned on initial UI load or refresh.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineSnapshot {
    pub version: String,
    pub devices: Vec<AudioDeviceInfo>,
    pub routes: Vec<RouteConfig>,
    pub profiles: Vec<Profile>,
    pub settings: AppSettings,
}

/// Hot-plug notification emitted by the OS audio backend.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "deviceId", rename_all = "camelCase")]
pub enum DeviceChangeEvent {
    Added(String),
    Removed(String),
    StateChanged(String),
    DefaultChanged,
}
