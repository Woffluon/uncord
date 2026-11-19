use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use ringbuf::traits::Consumer;

use crate::types::{AudioDeviceInfo, ChannelMode, DeviceFlow, LatencyMode, TransportKind};

/// Clipping threshold in normalized float space (`> 0.995` triggers clip indicator).
pub const CLIPPING_THRESHOLD: f32 = 0.995;

/// Decibel floor below which gain snaps to exact silence (`-59.5 dB`).
pub const SILENCE_FLOOR_DB: f32 = -59.5;

/// Maximum clock-drift rate adjustment in parts-per-million (`±2000 ppm`).
pub const MAX_DRIFT_PPM: f64 = 2000.0;

/// Converts decibels to linear amplitude, clamping `<= -59.5 dB` to exact zero.
#[inline]
pub fn gain_db_to_linear(db: f32) -> f32 {
    if !db.is_finite() || db <= SILENCE_FLOOR_DB {
        return 0.0;
    }
    let clamped = db.min(24.0);
    10.0_f32.powf(clamped / 20.0)
}

/// Lock-free atomic wrapper for `ChannelMode` used on the real-time capture path.
#[derive(Debug)]
pub struct AtomicChannelMode(AtomicU64);

impl AtomicChannelMode {
    pub fn new(mode: ChannelMode) -> Self {
        Self(AtomicU64::new(Self::encode(mode)))
    }

    #[inline]
    pub fn load(&self) -> ChannelMode {
        Self::decode(self.0.load(Ordering::Relaxed))
    }

    #[inline]
    pub fn store(&self, mode: ChannelMode) {
        self.0.store(Self::encode(mode), Ordering::Relaxed);
    }

    #[inline]
    const fn encode(mode: ChannelMode) -> u64 {
        match mode {
            ChannelMode::StereoPair { left, right } => {
                ((left as u64) << 16) | (right as u64)
            }
            ChannelMode::MonoToStereo { channel } => (1u64 << 56) | (channel as u64),
            ChannelMode::MixAllToStereo => 2u64 << 56,
        }
    }

    #[inline]
    const fn decode(raw: u64) -> ChannelMode {
        let tag = (raw >> 56) & 0xFF;
        match tag {
            1 => ChannelMode::MonoToStereo {
                channel: (raw & 0xFFFF) as u16,
            },
            2 => ChannelMode::MixAllToStereo,
            _ => ChannelMode::StereoPair {
                left: ((raw >> 16) & 0xFFFF) as u16,
                right: (raw & 0xFFFF) as u16,
            },
        }
    }
}

/// Lock-free atomic gain and mute controls shared between UI/engine and the render thread.
#[derive(Debug)]
pub struct AtomicGainControl {
    gain_db_bits: AtomicU32,
    muted: AtomicBool,
}

impl AtomicGainControl {
    pub fn new(gain_db: f32, muted: bool) -> Self {
        Self {
            gain_db_bits: AtomicU32::new(gain_db.to_bits()),
            muted: AtomicBool::new(muted),
        }
    }

    #[inline]
    pub fn set_gain_db(&self, gain_db: f32) {
        self.gain_db_bits.store(gain_db.to_bits(), Ordering::Relaxed);
    }

    #[inline]
    pub fn set_muted(&self, muted: bool) {
        self.muted.store(muted, Ordering::Relaxed);
    }

    #[inline]
    pub fn load(&self) -> (f32, bool) {
        let db = f32::from_bits(self.gain_db_bits.load(Ordering::Relaxed));
        let muted = self.muted.load(Ordering::Relaxed);
        (db, muted)
    }
}

/// Maps multi-channel interleaved capture frames (e.g. 4-channel USB amp) into stereo.
#[derive(Debug, Clone, Copy)]
pub struct ChannelMapper {
    mode: ChannelMode,
}

impl ChannelMapper {
    pub fn new(mode: ChannelMode) -> Self {
        Self { mode }
    }

    pub fn set_mode(&mut self, mode: ChannelMode) {
        self.mode = mode;
    }

    #[inline]
    pub fn map_frame(&self, frame: &[f32]) -> (f32, f32) {
        Self::map_f32_frame(self.mode, frame)
    }

    /// Maps one interleaved multi-channel `f32` frame to `(left, right)` with safe bounds clamping.
    #[inline]
    pub fn map_f32_frame(mode: ChannelMode, frame: &[f32]) -> (f32, f32) {
        let n = frame.len();
        if n == 0 {
            return (0.0, 0.0);
        }

        match mode {
            ChannelMode::StereoPair { left, right } => {
                let l_idx = (left as usize).min(n - 1);
                let r_idx = (right as usize).min(n - 1);
                (frame[l_idx], frame[r_idx])
            }
            ChannelMode::MonoToStereo { channel } => {
                let idx = (channel as usize).min(n - 1);
                let sample = frame[idx];
                (sample, sample)
            }
            ChannelMode::MixAllToStereo => {
                if n == 1 {
                    return (frame[0], frame[0]);
                }
                if n == 2 {
                    return (frame[0], frame[1]);
                }
                let mut sum_l = 0.0_f32;
                let mut sum_r = 0.0_f32;
                let mut count_l = 0_u32;
                let mut count_r = 0_u32;
                for (i, &sample) in frame.iter().enumerate() {
                    if i % 2 == 0 {
                        sum_l += sample;
                        count_l += 1;
                    } else {
                        sum_r += sample;
                        count_r += 1;
                    }
                }
                let l = if count_l > 0 {
                    sum_l / count_l as f32
                } else {
                    0.0
                };
                let r = if count_r > 0 {
                    sum_r / count_r as f32
                } else {
                    l
                };
                (l, r)
            }
        }
    }
}

/// Linear 2 ms ramp smoother preventing zipper noise on abrupt gain or mute changes.
#[derive(Debug, Clone)]
pub struct GainSmoother {
    ramp_samples: u32,
    current: f32,
    target: f32,
    step: f32,
    remaining: u32,
}

impl GainSmoother {
    pub fn new(sample_rate: u32, initial_gain_db: f32, initial_muted: bool) -> Self {
        let ramp_samples = ((sample_rate as f32 * 0.002).round() as u32).max(1);
        let initial = if initial_muted {
            0.0
        } else {
            gain_db_to_linear(initial_gain_db)
        };
        Self {
            ramp_samples,
            current: initial,
            target: initial,
            step: 0.0,
            remaining: 0,
        }
    }

    /// Updates the target gain; starts a smooth 2 ms linear ramp if the target changed.
    #[inline]
    pub fn set_target(&mut self, gain_db: f32, muted: bool) {
        let next_target = if muted { 0.0 } else { gain_db_to_linear(gain_db) };
        if (next_target - self.target).abs() > 1e-6 || (next_target == 0.0 && self.target != 0.0)
        {
            self.target = next_target;
            self.step = (next_target - self.current) / self.ramp_samples as f32;
            self.remaining = self.ramp_samples;
        }
    }

    /// Advances one frame and returns the smoothed linear multiplier.
    #[inline]
    pub fn next_gain(&mut self) -> f32 {
        if self.remaining > 0 {
            self.current += self.step;
            self.remaining -= 1;
            if self.remaining == 0 {
                self.current = self.target;
            }
        }
        self.current
    }

    #[inline]
    pub fn current_gain(&self) -> f32 {
        self.current
    }
}

/// Zero-allocation stereo Peak, RMS, and clipping detector for a single render callback.
#[derive(Debug, Default, Clone, Copy)]
pub struct LevelMeterAccumulator {
    peak_l: f32,
    peak_r: f32,
    sum_sq_l: f32,
    sum_sq_r: f32,
    clipping: bool,
    frames: u32,
}

impl LevelMeterAccumulator {
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    #[inline]
    pub fn push_frame(&mut self, left: f32, right: f32) {
        let abs_l = left.abs();
        let abs_r = right.abs();
        if abs_l > self.peak_l {
            self.peak_l = abs_l;
        }
        if abs_r > self.peak_r {
            self.peak_r = abs_r;
        }
        if abs_l > CLIPPING_THRESHOLD || abs_r > CLIPPING_THRESHOLD {
            self.clipping = true;
        }
        self.sum_sq_l += left * left;
        self.sum_sq_r += right * right;
        self.frames += 1;
    }

    /// Returns `(peak_left, peak_right, rms_left, rms_right, clipping)`.
    #[inline]
    pub fn finish(self) -> (f32, f32, f32, f32, bool) {
        if self.frames == 0 {
            return (0.0, 0.0, 0.0, 0.0, false);
        }
        let inv = 1.0 / self.frames as f32;
        let rms_l = (self.sum_sq_l * inv).sqrt().min(1.0);
        let rms_r = (self.sum_sq_r * inv).sqrt().min(1.0);
        (
            self.peak_l.min(1.0),
            self.peak_r.min(1.0),
            rms_l,
            rms_r,
            self.clipping,
        )
    }
}

/// Nominal SPSC ring-buffer target window in milliseconds for each latency mode.
#[inline]
pub const fn target_ring_buffer_ms(mode: LatencyMode) -> f32 {
    match mode {
        LatencyMode::UltraLow => 4.0,
        LatencyMode::Low => 8.0,
        LatencyMode::Balanced => 16.0,
        LatencyMode::Safe => 32.0,
    }
}

/// Precomputed SPSC ring-buffer geometry in stereo frames at capture sample rate.
#[derive(Debug, Clone, Copy)]
pub struct RingBufferLayout {
    pub target_frames: usize,
    pub capacity_frames: usize,
    pub target_ms: f32,
}

/// Computes SPSC target fill and total capacity ensuring headroom for both capture and render bursts.
pub fn compute_ring_buffer_layout(
    capture_sr: u32,
    capture_period_frames: u32,
    render_sr: u32,
    render_period_frames: u32,
    mode: LatencyMode,
) -> RingBufferLayout {
    let cap_sr = capture_sr.max(8000) as f64;
    let ren_sr = render_sr.max(8000) as f64;
    let mode_ms = target_ring_buffer_ms(mode) as f64;

    let cap_period_ms = (capture_period_frames as f64 / cap_sr) * 1000.0;
    let ren_period_ms = (render_period_frames as f64 / ren_sr) * 1000.0;

    // Ensure the target buffer depth is never smaller than one period of a fallback 10 ms driver.
    let max_period_ms = cap_period_ms.max(ren_period_ms);
    let effective_target_ms = mode_ms.max(max_period_ms * 1.05);

    let target_frames = ((effective_target_ms * cap_sr) / 1000.0).round().max(32.0) as usize;
    let render_period_in_cap =
        (render_period_frames as f64 * (cap_sr / ren_sr)).ceil() as usize;
    let max_period_frames = (capture_period_frames as usize).max(render_period_in_cap);

    let capacity_frames = (target_frames * 2)
        .max(max_period_frames * 3)
        .max(256);

    RingBufferLayout {
        target_frames,
        capacity_frames,
        target_ms: effective_target_ms as f32,
    }
}

/// Telemetry reported after resampling one render callback block.
#[derive(Debug, Clone, Copy)]
pub struct ResampleReport {
    pub fill_ratio: f32,
    pub ring_buffer_ms: f32,
    pub drift_ppm: f32,
    pub underrun: bool,
}

/// PI-controlled fractional 4-point cubic Hermite resampler locked to SPSC ring-buffer occupancy.
#[derive(Debug, Clone)]
pub struct AdaptiveResampler {
    capture_sr: u32,
    render_sr: u32,
    target_frames: usize,
    capacity_frames: usize,
    nominal_step: f64,
    current_step: f64,
    phase: f64,
    hist: [(f32, f32); 4],
    ema_stage1: f64,
    ema_fill_frames: f64,
    integral: f64,
    drift_ratio: f64,
    in_underrun: bool,
}

impl AdaptiveResampler {
    pub fn new(capture_sr: u32, render_sr: u32, layout: RingBufferLayout) -> Self {
        let cap_sr = capture_sr.max(8000);
        let ren_sr = render_sr.max(8000);
        let nominal_step = cap_sr as f64 / ren_sr as f64;
        Self {
            capture_sr: cap_sr,
            render_sr: ren_sr,
            target_frames: layout.target_frames,
            capacity_frames: layout.capacity_frames.max(1),
            nominal_step,
            current_step: nominal_step,
            phase: 0.0,
            hist: [(0.0, 0.0); 4],
            ema_stage1: layout.target_frames as f64,
            ema_fill_frames: layout.target_frames as f64,
            integral: 0.0,
            drift_ratio: 0.0,
            in_underrun: false,
        }
    }

    /// Returns the currently active drift compensation in parts-per-million.
    #[inline]
    pub fn drift_ppm(&self) -> f32 {
        (self.drift_ratio * 1_000_000.0) as f32
    }

    /// Consumes interleaved stereo `f32` samples from `consumer` and emits `out_frames` resampled frames.
    /// Executes zero heap allocations, locks, or I/O.
    #[inline]
    pub fn process_stereo_block<C, F>(
        &mut self,
        consumer: &mut C,
        out_frames: usize,
        mut emit_frame: F,
    ) -> ResampleReport
    where
        C: Consumer<Item = f32>,
        F: FnMut(usize, f32, f32),
    {
        if out_frames == 0 {
            let occ = consumer.occupied_len() / 2;
            return ResampleReport {
                fill_ratio: (occ as f32 / self.capacity_frames as f32).clamp(0.0, 1.0),
                ring_buffer_ms: (occ as f32 / self.capture_sr as f32) * 1000.0,
                drift_ppm: self.drift_ppm(),
                underrun: false,
            };
        }

        let occupied_before = consumer.occupied_len() / 2;
        let mut underrun = false;

        for frame_idx in 0..out_frames {
            while self.phase >= 1.0 {
                self.phase -= 1.0;
                let mut pair = [0.0_f32; 2];
                let popped = consumer.pop_slice(&mut pair);
                if popped == 2 {
                    self.shift_frame(pair[0], pair[1]);
                    self.in_underrun = false;
                } else {
                    if popped == 1 {
                        let _ = consumer.try_pop();
                    }
                    if !self.in_underrun {
                        underrun = true;
                        self.in_underrun = true;
                    }
                    let last = self.hist[3];
                    self.shift_frame(last.0 * 0.9, last.1 * 0.9);
                }
            }

            let t = self.phase as f32;
            let out_l = hermite4(
                self.hist[0].0,
                self.hist[1].0,
                self.hist[2].0,
                self.hist[3].0,
                t,
            );
            let out_r = hermite4(
                self.hist[0].1,
                self.hist[1].1,
                self.hist[2].1,
                self.hist[3].1,
                t,
            );
            emit_frame(frame_idx, out_l, out_r);
            self.phase += self.current_step;
        }

        let occupied_after = consumer.occupied_len() / 2;
        let avg_fill = 0.5 * (occupied_before as f64 + occupied_after as f64);
        let dt = (out_frames as f64) / (self.render_sr as f64);

        // Two-pole cascaded smoothing (120 ms per pole) rejects asynchronous callback beat sawtooth.
        let alpha = 1.0 - (-dt / 0.12).exp();
        self.ema_stage1 += alpha * (avg_fill - self.ema_stage1);
        self.ema_fill_frames += alpha * (self.ema_stage1 - self.ema_fill_frames);

        // Near-critically damped 2nd-order PI control (omega_n = 2.0 rad/s, zeta = 0.95).
        let error_sec = (self.ema_fill_frames - self.target_frames as f64) / (self.capture_sr as f64);
        const KP: f64 = 2.0 * 0.95 * 2.0;
        const KI: f64 = 2.0 * 2.0;
        const MAX_RATIO: f64 = MAX_DRIFT_PPM * 1e-6;

        let p_term = KP * error_sec;
        let next_integral = self.integral + KI * error_sec * dt;
        let unclamped = p_term + next_integral;

        // Anti-windup: freeze or clamp integration if output saturates at ±2000 ppm.
        if unclamped.abs() <= MAX_RATIO || (unclamped * error_sec) < 0.0 {
            self.integral = next_integral.clamp(-MAX_RATIO, MAX_RATIO);
        }

        self.drift_ratio = (p_term + self.integral).clamp(-MAX_RATIO, MAX_RATIO);
        self.current_step = self.nominal_step * (1.0 + self.drift_ratio);

        ResampleReport {
            fill_ratio: (self.ema_fill_frames / self.capacity_frames as f64).clamp(0.0, 1.0) as f32,
            ring_buffer_ms: ((self.ema_fill_frames / self.capture_sr as f64) * 1000.0) as f32,
            drift_ppm: self.drift_ppm(),
            underrun,
        }
    }

    #[inline]
    fn shift_frame(&mut self, left: f32, right: f32) {
        self.hist[0] = self.hist[1];
        self.hist[1] = self.hist[2];
        self.hist[2] = self.hist[3];
        self.hist[3] = (left, right);
    }
}

/// 4-point, 3rd-order C1-continuous cubic Hermite interpolation.
#[inline]
fn hermite4(y0: f32, y1: f32, y2: f32, y3: f32, t: f32) -> f32 {
    let c0 = y1;
    let c1 = 0.5 * (y2 - y0);
    let c2 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
    let c3 = 0.5 * (y3 - y0) + 1.5 * (y1 - y2);
    ((c3 * t + c2) * t + c1) * t + c0
}

/// Classifies an audio endpoint's physical/virtual transport and whether it carries a
/// low-bandwidth Bluetooth HFP/Hands-Free quality & latency warning.
pub fn classify_transport(
    friendly_name: &str,
    enumerator: Option<&str>,
    channels: u16,
    sample_rate: u32,
) -> (TransportKind, bool) {
    let name = friendly_name.to_ascii_lowercase();
    let enum_upper = enumerator.unwrap_or("").to_ascii_uppercase();

    let is_hfp_name = name.contains("hands-free")
        || name.contains("handsfree")
        || name.contains("ag audio")
        || name.contains("hfp");
    let is_bt_enum = enum_upper.contains("BTHENUM")
        || enum_upper.contains("BTHHFENUM")
        || enum_upper.contains("BTHA2DP")
        || enum_upper.contains("BTHLE");
    let is_bt_name = name.contains("bluetooth")
        || name.contains("a2dp")
        || name.contains("wh-")
        || name.contains("wf-")
        || name.contains("airpods")
        || name.contains("buds");

    if enum_upper.contains("BTHHFENUM") || is_hfp_name {
        return (TransportKind::BluetoothHfp, true);
    }

    if (is_bt_enum || is_bt_name) && channels == 1 && sample_rate <= 16_000 {
        return (TransportKind::BluetoothHfp, true);
    }

    if is_bt_enum || is_bt_name {
        return (TransportKind::BluetoothA2dp, false);
    }

    if name.contains("vb-audio")
        || name.contains("voicemeeter")
        || name.contains("virtual")
        || name.contains("cable ")
        || name.contains("cable input")
        || name.contains("cable output")
        || name.contains("sonar")
        || name.contains("wave link")
        || name.contains("blackhole")
    {
        return (TransportKind::VirtualCable, false);
    }

    if name.contains("2.4ghz")
        || name.contains("2.4 ghz")
        || name.contains("lightspeed")
        || name.contains("hyperspeed")
        || name.contains("slipstream")
        || name.contains("dongle")
        || name.contains("wireless")
    {
        return (TransportKind::Wireless24Ghz, false);
    }

    if enum_upper.contains("USB")
        || name.contains("usb")
        || name.contains("blackstar")
        || name.contains("scarlett")
        || name.contains("focusrite")
        || name.contains("steinberg")
        || name.contains("behringer")
        || name.contains("rode")
        || name.contains("dac")
    {
        return (TransportKind::Usb, false);
    }

    if enum_upper.contains("HDAUDIO")
        || name.contains("realtek")
        || name.contains("high definition audio")
        || name.contains("speakers")
        || name.contains("headphones")
        || name.contains("monitors")
        || name.contains("line in")
        || name.contains("line out")
        || name.contains("hdmi")
        || name.contains("displayport")
        || name.contains("nvidia")
        || name.contains("amd")
        || name.contains("wired")
    {
        return (TransportKind::Wired, false);
    }

    (TransportKind::Unknown, false)
}

/// Baseline physical transport hardware/codec delay floor in milliseconds.
#[inline]
pub const fn estimated_transport_floor_ms(transport: TransportKind) -> f32 {
    match transport {
        TransportKind::Wired | TransportKind::VirtualCable => 0.5,
        TransportKind::Usb => 2.0,
        TransportKind::Wireless24Ghz => 12.0,
        TransportKind::BluetoothA2dp => 150.0,
        TransportKind::BluetoothHfp => 40.0,
        TransportKind::Unknown => 5.0,
    }
}

/// Returns `true` when `device` matches a route's source device ID and loopback flag.
pub(crate) fn matches_source_device(
    device: &AudioDeviceInfo,
    source_device_id: &str,
    is_loopback: bool,
) -> bool {
    let canonical = source_device_id
        .strip_prefix("loopback::")
        .unwrap_or(source_device_id);

    if is_loopback {
        let is_loopback_match = device.flow == DeviceFlow::Loopback
            && (device.id == source_device_id
                || device.id.strip_prefix("loopback::") == Some(canonical));
        let is_render_match = device.flow == DeviceFlow::Render && device.id == canonical;
        is_loopback_match || is_render_match
    } else {
        device.flow == DeviceFlow::Capture && device.id == source_device_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ringbuf::traits::{Producer, Split};
    use ringbuf::HeapRb;

    #[test]
    fn channel_mapper_handles_4ch_and_clamping() {
        let frame = [0.1, 0.2, 0.3, 0.4];

        assert_eq!(
            ChannelMapper::map_f32_frame(ChannelMode::StereoPair { left: 0, right: 1 }, &frame),
            (0.1, 0.2)
        );
        assert_eq!(
            ChannelMapper::map_f32_frame(ChannelMode::StereoPair { left: 2, right: 3 }, &frame),
            (0.3, 0.4)
        );
        assert_eq!(
            ChannelMapper::map_f32_frame(ChannelMode::MonoToStereo { channel: 3 }, &frame),
            (0.4, 0.4)
        );
        // Out-of-bounds indices clamp cleanly to the highest channel.
        assert_eq!(
            ChannelMapper::map_f32_frame(ChannelMode::StereoPair { left: 6, right: 9 }, &frame),
            (0.4, 0.4)
        );
        assert_eq!(
            ChannelMapper::map_f32_frame(ChannelMode::MonoToStereo { channel: 12 }, &[0.75]),
            (0.75, 0.75)
        );
        let mixed = ChannelMapper::map_f32_frame(ChannelMode::MixAllToStereo, &frame);
        assert!((mixed.0 - 0.2).abs() < 1e-6);
        assert!((mixed.1 - 0.3).abs() < 1e-6);
        assert_eq!(
            ChannelMapper::map_f32_frame(ChannelMode::MixAllToStereo, &[]),
            (0.0, 0.0)
        );
    }

    #[test]
    fn atomic_channel_mode_roundtrips_all_variants() {
        let modes = [
            ChannelMode::StereoPair { left: 0, right: 1 },
            ChannelMode::StereoPair { left: 2, right: 3 },
            ChannelMode::MonoToStereo { channel: 3 },
            ChannelMode::MixAllToStereo,
        ];
        let atom = AtomicChannelMode::new(modes[0]);
        for m in modes {
            atom.store(m);
            assert_eq!(atom.load(), m);
        }
    }

    #[test]
    fn gain_db_silence_threshold_and_ramp_smoothing() {
        assert_eq!(gain_db_to_linear(-59.5), 0.0);
        assert_eq!(gain_db_to_linear(-60.0), 0.0);
        assert_eq!(gain_db_to_linear(f32::NAN), 0.0);
        assert!((gain_db_to_linear(0.0) - 1.0).abs() < 1e-6);
        assert!((gain_db_to_linear(-6.0) - 0.5011872).abs() < 1e-5);

        // 2 ms at 48 kHz = 96 samples
        let mut smoother = GainSmoother::new(48_000, 0.0, false);
        assert!((smoother.current_gain() - 1.0).abs() < 1e-6);

        smoother.set_target(0.0, true);
        let mut prev = 1.0_f32;
        for _ in 0..95 {
            let g = smoother.next_gain();
            assert!(g < prev && g > 0.0, "expected monotonic ramp down, got {g}");
            prev = g;
        }
        assert_eq!(smoother.next_gain(), 0.0);
    }

    #[test]
    fn level_meter_detects_clipping_and_rms() {
        let mut acc = LevelMeterAccumulator::new();
        acc.push_frame(0.5, -0.5);
        acc.push_frame(0.996, 0.25);
        let (pl, pr, rl, rr, clip) = acc.finish();
        assert!((pl - 0.996).abs() < 1e-6);
        assert!((pr - 0.5).abs() < 1e-6);
        assert!(clip);
        assert!(rl > 0.7 && rr > 0.3);
    }

    #[test]
    fn resampler_tracks_negative_and_positive_drift_without_xrun() {
        let cap_sr = 48_000_u32;
        let ren_sr = 48_000_u32;
        let layout = compute_ring_buffer_layout(cap_sr, 128, ren_sr, 128, LatencyMode::UltraLow);
        let rb = HeapRb::<f32>::new(layout.capacity_frames * 2);
        let (mut prod, mut cons) = rb.split();
        for _ in 0..(layout.target_frames * 2) {
            let _ = prod.try_push(0.0);
        }

        let mut resampler = AdaptiveResampler::new(cap_sr, ren_sr, layout);
        // Simulate -180 ppm slower capture crystal over 5 seconds.
        let total_callbacks = (5 * 48_000) / 128;
        let mut cap_accum = 0.0_f64;
        let cap_per_cb = 128.0 * (1.0 - 180e-6);
        let mut xruns = 0_u64;
        let mut last_report = None;

        for _ in 0..total_callbacks {
            cap_accum += cap_per_cb;
            let to_push = cap_accum.floor() as usize;
            cap_accum -= to_push as f64;
            for _ in 0..to_push {
                if prod.push_slice(&[0.25, -0.25]) < 2 {
                    xruns += 1;
                }
            }
            let r = resampler.process_stereo_block(&mut cons, 128, |_, _, _| {});
            if r.underrun {
                xruns += 1;
            }
            last_report = Some(r);
        }

        let report = last_report.unwrap();
        assert_eq!(xruns, 0);
        assert!(
            (report.drift_ppm - (-180.0)).abs() < 25.0,
            "expected drift_ppm near -180, got {}",
            report.drift_ppm
        );
    }

    #[test]
    fn transport_classification_and_hfp_warning() {
        assert_eq!(
            classify_transport("Blackstar ID:Core V4 (USB Audio)", Some("USB"), 4, 48_000),
            (TransportKind::Usb, false)
        );
        assert_eq!(
            classify_transport("WH-1000XM5 Stereo (Bluetooth)", Some("BTHENUM"), 2, 44_100),
            (TransportKind::BluetoothA2dp, false)
        );
        assert_eq!(
            classify_transport("WH-1000XM5 Hands-Free AG Audio", None, 1, 16_000),
            (TransportKind::BluetoothHfp, true)
        );
        assert_eq!(
            classify_transport("Studio Monitors (Wired)", None, 2, 48_000),
            (TransportKind::Wired, false)
        );
        assert_eq!(
            classify_transport("CABLE Input (VB-Audio Virtual Cable)", None, 2, 48_000),
            (TransportKind::VirtualCable, false)
        );
    }
}
