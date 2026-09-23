//! 独立进程运行实时 DSP；兼顾麦克风正常包和启动积压的大包。
use super::*;
use crate::performance_test_support::memory;
use std::time::Instant;

#[test]
#[ignore = "独立性能采样：release、单进程，不调用识别服务"]
fn realtime_dsp_memory_profile() {
    let number = |key: &str, default: usize| {
        std::env::var(key)
            .ok()
            .map(|v| v.parse::<usize>().expect("性能参数必须是整数"))
            .unwrap_or(default)
    };
    let seconds = number("SAYIT_PERF_AUDIO_SECONDS", 60);
    let rate = number("SAYIT_PERF_SAMPLE_RATE", 48_000);
    let packet_size = number("SAYIT_PERF_PACKET_SIZE", 4096);
    assert!((1..=1800).contains(&seconds));
    assert!((8_000..=192_000).contains(&rate));
    assert!((1..=2_880_000).contains(&packet_size));
    let denoise = std::env::var("SAYIT_PERF_DENOISE").as_deref() == Ok("1");
    let initial = memory();
    let input: Vec<f32> = (0..packet_size.min(seconds * rate))
        .map(|i| ((i % 997) as f32 / 996.0 - 0.5) * 0.2)
        .collect();
    let mut dsp = StreamDsp::new(
        DspParams {
            denoise_enabled: denoise,
            ..Default::default()
        },
        rate as u32,
    );
    let before = memory();
    let started = Instant::now();
    let mut output_bytes = 0;
    let mut hash = 0xcbf29ce484222325_u64;
    for offset in (0..seconds * rate).step_by(packet_size) {
        let output = dsp.process(&input[..(seconds * rate - offset).min(packet_size)]);
        output_bytes += output.len();
        for value in output {
            hash = (hash ^ value as u64).wrapping_mul(0x100000001b3);
        }
    }
    let elapsed = started.elapsed();
    let after = memory();
    // 非 48k 的连续线性插值会保留不足一帧的尾部，不人为补零。
    assert!(output_bytes <= seconds * 16_000 * 2);
    assert!(output_bytes + 320 >= seconds * 16_000 * 2);
    drop(dsp);
    drop(input);
    let released = memory();
    println!(
        "PERF_RESULT {}",
        serde_json::json!({
            "scenario": "realtime-dsp", "seconds": seconds, "sampleRate": rate,
            "packetSize": packet_size, "denoise": denoise,
            "elapsedMs": elapsed.as_secs_f64() * 1000.0,
            "outputHash": format!("{hash:016x}"), "outputBytes": output_bytes,
            "initialPrivateBytes": initial.private_usage, "inputPrivateBytes": before.private_usage,
            "retainedPrivateBytes": after.private_usage, "peakPrivateBytes": after.peak_pagefile_usage,
            "peakWorkingSetBytes": after.peak_working_set, "releasedPrivateBytes": released.private_usage,
        })
    );
}
