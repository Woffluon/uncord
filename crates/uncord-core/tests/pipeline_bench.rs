use std::time::Instant;

use ringbuf::traits::{Producer, Split};
use ringbuf::HeapRb;
use uncord_core::dsp::{
    compute_ring_buffer_layout, AdaptiveResampler, ChannelMapper, GainSmoother,
    LevelMeterAccumulator,
};
use uncord_core::types::{ChannelMode, LatencyMode};

#[test]
fn bench_pipeline_latency_and_drift() {
    let cap_sr = 48_000_u32;
    let ren_sr = 44_100_u32;
    let cap_period_frames = 128_u32; // 2.667 ms @ 48 kHz
    let ren_period_frames = 128_u32; // 2.902 ms @ 44.1 kHz
    let true_drift_ppm = 150.0_f64;

    let layout = compute_ring_buffer_layout(
        cap_sr,
        cap_period_frames,
        ren_sr,
        ren_period_frames,
        LatencyMode::UltraLow,
    );

    let rb = HeapRb::<f32>::new(layout.capacity_frames * 2);
    let (mut prod, mut cons) = rb.split();
    for _ in 0..(layout.target_frames * 2) {
        let _ = prod.try_push(0.0);
    }

    let mapper = ChannelMapper::new(ChannelMode::StereoPair { left: 2, right: 3 });
    let mut resampler = AdaptiveResampler::new(cap_sr, ren_sr, layout);
    let mut smoother = GainSmoother::new(ren_sr, 0.0, false);

    let cap_interval_sec =
        cap_period_frames as f64 / (cap_sr as f64 * (1.0 + true_drift_ppm * 1e-6));
    let ren_interval_sec = ren_period_frames as f64 / ren_sr as f64;
    let sim_duration_sec = 5.0_f64;

    let mut next_cap_sec = cap_interval_sec;
    let mut next_ren_sec = ren_interval_sec;
    let mut sample_phase = 0_u64;
    let mut xruns = 0_u64;
    let mut last_report = None;
    let mut last_rms = 0.0_f32;
    let mut stack_buf = [0.0_f32; 256];

    let wall_start = Instant::now();

    while next_cap_sec <= sim_duration_sec || next_ren_sec <= sim_duration_sec {
        if next_cap_sec <= next_ren_sec {
            let mut batch_len = 0_usize;
            for _ in 0..cap_period_frames {
                let t = sample_phase as f64 / cap_sr as f64;
                sample_phase += 1;
                let frame4 = [
                    0.1 * (std::f64::consts::TAU * 100.0 * t).sin() as f32,
                    0.1 * (std::f64::consts::TAU * 150.0 * t).sin() as f32,
                    0.4 * (std::f64::consts::TAU * 440.0 * t).sin() as f32,
                    0.4 * (std::f64::consts::TAU * 660.0 * t).sin() as f32,
                ];
                let (l, r) = mapper.map_frame(&frame4);
                stack_buf[batch_len] = l;
                stack_buf[batch_len + 1] = r;
                batch_len += 2;
            }
            if prod.push_slice(&stack_buf[..batch_len]) < batch_len {
                xruns += 1;
            }
            next_cap_sec += cap_interval_sec;
        } else {
            smoother.set_target(0.0, false);
            let mut meter = LevelMeterAccumulator::new();
            let report = resampler.process_stereo_block(
                &mut cons,
                ren_period_frames as usize,
                |_, l, r| {
                    let g = smoother.next_gain();
                    meter.push_frame(l * g, r * g);
                },
            );
            if report.underrun {
                xruns += 1;
            }
            let (_, _, rl, rr, _) = meter.finish();
            last_rms = 0.5 * (rl + rr);
            last_report = Some(report);
            next_ren_sec += ren_interval_sec;
        }
    }

    let wall_elapsed = wall_start.elapsed();
    let report = last_report.expect("render must execute callbacks");
    let capture_period_ms = (cap_period_frames as f32 / cap_sr as f32) * 1000.0;
    let render_period_ms = (ren_period_frames as f32 / ren_sr as f32) * 1000.0;
    let software_total_ms = capture_period_ms + report.ring_buffer_ms + render_period_ms;

    println!(
        "bench_pipeline_latency_and_drift: simulated=5.00s wall_ms={:.3} capture_ms={:.3} ring_ms={:.3} render_ms={:.3} software_total_ms={:.3} drift_ppm={:.2} fill_ratio={:.3} xruns={} rms={:.4}",
        wall_elapsed.as_secs_f64() * 1000.0,
        capture_period_ms,
        report.ring_buffer_ms,
        render_period_ms,
        software_total_ms,
        report.drift_ppm,
        report.fill_ratio,
        xruns,
        last_rms,
    );

    assert_eq!(xruns, 0, "expected 0 xruns over 5 simulated seconds");
    assert!(
        (report.drift_ppm - 150.0).abs() <= 15.0,
        "expected drift_ppm to converge near +150.0 ppm, got {:.2}",
        report.drift_ppm
    );
    assert!(
        software_total_ms <= 12.0,
        "expected software_total_ms <= 12.0 ms, got {:.3} ms",
        software_total_ms
    );
    assert!(last_rms > 0.1, "expected valid audio signal output, got rms={last_rms}");
}
