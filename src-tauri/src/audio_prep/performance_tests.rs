//! 解码后立即消费的本地文件负载，独立进程衡量峰值，散列验证音频未发生变化。
use super::*;
use crate::performance_test_support::memory;
use std::time::Instant;

#[test]
#[ignore = "独立性能采样：release 模式、单用例、单线程，不调用真实服务"]
fn file_decode_memory_profile() {
    let seconds = std::env::var("SAYIT_PERF_AUDIO_SECONDS")
        .ok()
        .map(|value| value.parse::<usize>().expect("音频时长必须是整数"))
        .unwrap_or(300);
    assert!((1..=1800).contains(&seconds));
    let path = std::env::temp_dir().join(format!("say-it-decode-{}.wav", uuid::Uuid::new_v4()));
    write_test_stereo_wav(&path, seconds as f32, 48_000);
    let initial = memory();
    let started = Instant::now();
    let mut output_hash = 0xcbf29ce484222325_u64;
    let count = decode_mono_16k_chunks(
        path.to_str().unwrap(),
        || Ok(()),
        |chunk| {
            for value in chunk {
                output_hash =
                    (output_hash ^ u64::from(value.to_bits())).wrapping_mul(0x100000001b3);
            }
            Ok(())
        },
    )
    .unwrap();
    let elapsed = started.elapsed();
    let after = memory();
    let released = memory();
    std::fs::remove_file(path).unwrap();
    assert_eq!(count, seconds as u64 * u64::from(TARGET_SAMPLE_RATE));
    println!(
        "PERF_RESULT {}",
        serde_json::json!({
            "scenario": "file-decode",
            "seconds": seconds,
            "sampleCount": count,
            "elapsedMs": elapsed.as_secs_f64() * 1000.0,
            "outputHash": format!("{output_hash:016x}"),
            "initialPrivateBytes": initial.private_usage,
            "retainedPrivateBytes": after.private_usage,
            "peakPrivateBytes": after.peak_pagefile_usage,
            "peakWorkingSetBytes": after.peak_working_set,
            "releasedPrivateBytes": released.private_usage,
        })
    );
}
