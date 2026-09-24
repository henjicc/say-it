//! 在独立的 release 测试进程中运行，避免其他用例污染进程峰值。
use super::*;
use crate::performance_test_support::memory;
use std::time::Instant;

#[test]
#[ignore = "独立性能采样：release 模式、单用例、单线程，不调用真实服务"]
fn offline_audio_memory_profile() {
    let seconds = std::env::var("SAYIT_PERF_AUDIO_SECONDS")
        .ok()
        .map(|value| value.parse::<usize>().expect("音频时长必须是整数"))
        .unwrap_or(300);
    assert!((1..=1800).contains(&seconds));
    let denoise = std::env::var("SAYIT_PERF_DENOISE").as_deref() == Ok("1");
    use windows::Win32::System::Threading::{
        GetCurrentProcess, GetProcessHandleCount, GetProcessIoCounters, IO_COUNTERS,
    };
    let handles = || {
        let mut count = 0;
        unsafe { GetProcessHandleCount(GetCurrentProcess(), &mut count).unwrap() };
        count
    };
    let io = || {
        let mut counters = IO_COUNTERS::default();
        unsafe { GetProcessIoCounters(GetCurrentProcess(), &mut counters).unwrap() };
        counters
    };
    let initial = memory();
    let io_initial = io();
    let handles_initial = handles();
    let recording_started = Instant::now();
    let runtime = AudioLabRuntime::default();
    runtime.begin(48_000).unwrap();
    // 分块生成本地测试信号，避免完整输入副本计入待测链路。
    let chunk: Vec<f32> = (0..480)
        .map(|index| (index as f32 / 480.0 - 0.5) * 0.2)
        .collect();
    for _ in 0..seconds * 100 {
        runtime.append(&chunk);
    }
    runtime.stop().unwrap();
    let recording_elapsed = recording_started.elapsed();
    let io_recorded = io();
    let before = memory();
    let started = Instant::now();
    let snapshot = runtime
        .reprocess(DspParams {
            denoise_enabled: denoise,
            ..Default::default()
        })
        .unwrap();
    let elapsed = started.elapsed();
    let after = memory();
    let io_processed = io();
    let handles_processed = handles();
    let output_hash = {
        let state = runtime.state.lock().unwrap();
        assert_eq!(state.processed.len(), seconds * 48_000);
        let mut hash = 0xcbf29ce484222325_u64;
        state
            .processed
            .visit(|samples| {
                for value in samples {
                    hash = (hash ^ u64::from(value.to_bits())).wrapping_mul(0x100000001b3);
                }
                Ok(())
            })
            .unwrap();
        hash
    };
    assert_eq!(snapshot.duration_ms, seconds as u64 * 1000);
    drop(runtime);
    let released = memory();
    let handles_released = handles();
    assert_eq!(handles_released, handles_initial);
    println!(
        "PERF_RESULT {}",
        serde_json::json!({
            "scenario": "audio-lab-offline",
            "recordingMs": recording_elapsed.as_secs_f64() * 1000.0,
            "recordingWriteBytes": io_recorded.WriteTransferCount - io_initial.WriteTransferCount,
            "processingReadBytes": io_processed.ReadTransferCount - io_recorded.ReadTransferCount,
            "processingWriteBytes": io_processed.WriteTransferCount - io_recorded.WriteTransferCount,
            "initialHandles": handles_initial, "retainedHandles":handles_processed, "releasedHandles":handles_released,
            "seconds": seconds,
            "denoise": denoise,
            "elapsedMs": elapsed.as_secs_f64() * 1000.0,
            "outputHash": format!("{output_hash:016x}"),
            "stats": snapshot.stats,
            "initialPrivateBytes": initial.private_usage,
            "inputPrivateBytes": before.private_usage,
            "retainedPrivateBytes": after.private_usage,
            "peakPrivateBytes": after.peak_pagefile_usage,
            "peakWorkingSetBytes": after.peak_working_set,
            "releasedPrivateBytes": released.private_usage,
        })
    );
}
