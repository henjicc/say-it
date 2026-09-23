//! 在独立的 release 测试进程中运行，避免其他用例污染进程峰值。
use super::*;
use std::ffi::c_void;
use std::time::Instant;

#[repr(C)]
#[derive(Default)]
struct MemoryCounters {
    size: u32,
    page_fault_count: u32,
    peak_working_set: usize,
    working_set: usize,
    peak_paged_pool: usize,
    paged_pool: usize,
    peak_nonpaged_pool: usize,
    nonpaged_pool: usize,
    pagefile_usage: usize,
    peak_pagefile_usage: usize,
    private_usage: usize,
}

#[link(name = "kernel32")]
extern "system" {
    fn GetCurrentProcess() -> *mut c_void;
    fn K32GetProcessMemoryInfo(
        process: *mut c_void,
        counters: *mut MemoryCounters,
        size: u32,
    ) -> i32;
}

fn memory() -> MemoryCounters {
    let mut counters = MemoryCounters {
        size: std::mem::size_of::<MemoryCounters>() as u32,
        ..Default::default()
    };
    // 使用当前进程伪句柄；结构与 Windows PROCESS_MEMORY_COUNTERS_EX 一致。
    let size = counters.size;
    assert_ne!(
        unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, size) },
        0
    );
    counters
}

#[test]
#[ignore = "独立性能采样：release 模式、单用例、单线程，不调用真实服务"]
fn offline_audio_memory_profile() {
    let seconds = std::env::var("SAYIT_PERF_AUDIO_SECONDS")
        .ok()
        .map(|value| value.parse::<usize>().expect("音频时长必须是整数"))
        .unwrap_or(300);
    assert!((1..=1800).contains(&seconds));
    let denoise = std::env::var("SAYIT_PERF_DENOISE").as_deref() == Ok("1");
    let initial = memory();
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
    let output_hash = {
        let state = runtime.state.lock().unwrap();
        assert_eq!(state.processed.len(), seconds * 48_000);
        state
            .processed
            .iter()
            .fold(0xcbf29ce484222325_u64, |hash, value| {
                (hash ^ u64::from(value.to_bits())).wrapping_mul(0x100000001b3)
            })
    };
    assert_eq!(snapshot.duration_ms, seconds as u64 * 1000);
    drop(runtime);
    let released = memory();
    println!(
        "PERF_RESULT {}",
        serde_json::json!({
            "scenario": "audio-lab-offline",
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
