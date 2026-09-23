//! 单独进程测量真实导出路径；不启动模型或网络请求。
use super::*;
use crate::performance_test_support::memory;
use std::io::Read;
use std::time::Instant;

#[test]
#[ignore = "独立性能采样：release 模式、单用例、单线程"]
fn wav_export_memory_profile() {
    let seconds = std::env::var("SAYIT_PERF_AUDIO_SECONDS")
        .ok()
        .map(|v| v.parse::<usize>().expect("音频时长必须是整数"))
        .unwrap_or(300);
    assert!((1..=1800).contains(&seconds));
    let initial = memory();
    let samples: Vec<f32> = (0..seconds * 48_000)
        .map(|i| ((i % 997) as f32 / 996.0 - 0.5) * 2.5)
        .collect();
    let before = memory();
    let started = Instant::now();
    let path = write_wav(&samples, 48_000).unwrap();
    let elapsed = started.elapsed();
    let after = memory();
    let mut file = std::fs::File::open(&path).unwrap();
    let output_bytes = file.metadata().unwrap().len();
    assert_eq!(output_bytes, 44 + samples.len() as u64 * 2);
    let mut buffer = [0u8; 64 * 1024];
    let mut hash = 0xcbf29ce484222325_u64;
    loop {
        let n = file.read(&mut buffer).unwrap();
        if n == 0 {
            break;
        }
        for byte in &buffer[..n] {
            hash = (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
        }
    }
    drop(file);
    std::fs::remove_file(path).unwrap();
    drop(samples);
    let released = memory();
    println!(
        "PERF_RESULT {}",
        serde_json::json!({
            "scenario": "compare-wav-export",
            "seconds": seconds,
            "elapsedMs": elapsed.as_secs_f64() * 1000.0,
            "outputBytes": output_bytes,
            "outputHash": format!("{hash:016x}"),
            "initialPrivateBytes": initial.private_usage,
            "inputPrivateBytes": before.private_usage,
            "retainedPrivateBytes": after.private_usage,
            "peakPrivateBytes": after.peak_pagefile_usage,
            "peakWorkingSetBytes": after.peak_working_set,
            "releasedPrivateBytes": released.private_usage,
        })
    );
}
