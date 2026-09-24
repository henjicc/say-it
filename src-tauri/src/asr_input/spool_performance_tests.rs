use super::*;
use crate::performance_test_support::memory;
use std::time::{Duration, Instant};

// cae5ec6 的实时发送计费和队列封装，保留用于同进程条件的算法对照。
#[derive(Default)]
struct LegacyBudget {
    bytes: AtomicUsize,
    space: Notify,
}
struct LegacyInput {
    input: Option<AsrStreamInput>,
    budget: Arc<LegacyBudget>,
    bytes: usize,
}
impl Drop for LegacyInput {
    fn drop(&mut self) {
        self.budget.bytes.fetch_sub(self.bytes, Ordering::AcqRel);
        self.budget.space.notify_waiters();
    }
}

#[test]
#[ignore = "独立实时队列测量：合成音频、真实临时文件，不调用设备或识别服务"]
fn live_queue_profile() {
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
    let seconds: usize = std::env::var("SAYIT_PERF_AUDIO_SECONDS")
        .unwrap()
        .parse()
        .unwrap();
    assert!((1..=1800).contains(&seconds));
    let legacy = std::env::var("SAYIT_PERF_SPOOL_LEGACY").as_deref() == Ok("1");
    let short = std::env::var("SAYIT_PERF_SPOOL_SHORT").as_deref() == Ok("1");
    let (handle, mut receiver) = AsrStreamHandle::channel();
    let (old_tx, mut old_rx) = mpsc::unbounded_channel::<LegacyInput>();
    let old_budget = Arc::new(LegacyBudget::default());
    let old_cancelled = AtomicBool::new(false);
    let mut receive = || {
        if legacy {
            assert!(!old_cancelled.load(Ordering::Acquire));
            old_rx
                .blocking_recv()
                .map(|mut queued| queued.input.take().unwrap())
        } else {
            receiver.blocking_recv()
        }
    };
    let input: Vec<f32> = (0..4096).map(|i| (i % 997) as f32 / 997.0 - 0.5).collect();
    let mut hash = 0xcbf29ce484222325_u64;
    let mut count = 0;
    let mut consume = |item| {
        let Some(AsrStreamInput::RawF32(samples)) = item else {
            panic!("音频不能丢失或提前结束")
        };
        count += samples.len();
        for sample in samples {
            hash = (hash ^ sample.to_bits() as u64).wrapping_mul(0x100000001b3);
        }
    };
    // 桌面宿主本已有 Tokio；两边预先初始化相同运行时，避免将线程池启动算成读盘成本。
    tauri::async_runtime::block_on(async {});
    let initial = memory();
    let initial_handles = handles();
    let io_before = io();
    let started = Instant::now();
    let mut send_time = Duration::ZERO;
    let mut max_resident = 0;
    let mut max_disk = 0;
    let mut packets = 0;
    for offset in (0..seconds * 48_000).step_by(4096) {
        // 长积压以 100 倍实时、每 16 包一批注入。消费者直到全部提交后才开始读取。
        // 两种实现使用完全相同的节奏，设备发送路径本身没有等待或写盘。
        if !short && packets % 16 == 0 {
            let target = Duration::from_secs_f64(offset as f64 / 48_000.0 / 100.0);
            if let Some(wait) = target.checked_sub(started.elapsed()) {
                std::thread::sleep(wait);
            }
        }
        let samples = input[..(seconds * 48_000 - offset).min(4096)].to_vec();
        let sending = Instant::now();
        if legacy {
            assert!(!old_cancelled.load(Ordering::Acquire));
            let bytes = std::mem::size_of::<LegacyInput>() + samples.capacity() * 4;
            old_budget.bytes.fetch_add(bytes, Ordering::AcqRel);
            assert!(old_tx
                .send(LegacyInput {
                    input: Some(AsrStreamInput::RawF32(samples)),
                    budget: old_budget.clone(),
                    bytes
                })
                .is_ok());
        } else {
            handle.tx.send(AsrStreamInput::RawF32(samples)).unwrap();
        }
        send_time += sending.elapsed();
        packets += 1;
        max_resident = max_resident.max(handle.tx.budget.resident.load(Ordering::Acquire));
        max_disk = max_disk.max(handle.tx.spool.retained_disk_bytes());
        if short {
            consume(receive());
        }
    }
    let enqueued = started.elapsed();
    let retained = memory();
    let retained_handles = handles();
    if !short {
        for _ in 0..packets {
            consume(receive());
        }
    }
    drop(consume);
    drop(receive);
    let read_done = started.elapsed();
    drop(receiver);
    drop(old_rx);
    let cleanup_started = Instant::now();
    let paths = handle.tx.spool.test_paths();
    while handle.tx.budget.resident.load(Ordering::Acquire) != 0
        || paths.iter().any(|path| path.exists())
    {
        assert!(cleanup_started.elapsed() < Duration::from_secs(5));
        std::thread::sleep(Duration::from_millis(1));
    }
    let after = memory();
    let released_handles = handles();
    let io_after = io();
    assert_eq!(count, seconds * 48_000);
    assert_eq!(handle.tx.budget.bytes.load(Ordering::Acquire), 0);
    assert_eq!(old_budget.bytes.load(Ordering::Acquire), 0);
    assert!(max_resident <= Limits::default().resident);
    if short {
        assert!(paths.is_empty());
    }
    println!(
        "PERF_RESULT {}",
        serde_json::json!({
            "scenario":"live-asr-queue","legacy":legacy,"shortQueue":short,"seconds":seconds,
            "acceleration":if short{None}else{Some(100)},"packets":packets,"samples":count,
            "elapsedMs":started.elapsed().as_secs_f64()*1000.0,"enqueueMs":enqueued.as_secs_f64()*1000.0,
        "sendPathMs":send_time.as_secs_f64()*1000.0,"drainMs":(read_done-enqueued).as_secs_f64()*1000.0,
            "cleanupMs":cleanup_started.elapsed().as_secs_f64()*1000.0,
        "maxObservedResidentAudioBytes":max_resident,"createdSegments":paths.len(),
        "maxObservedDiskBytes":max_disk,
        "initialPrivateBytes":initial.private_usage,"retainedPrivateBytes":retained.private_usage,
        "initialHandles":initial_handles,"retainedHandles":retained_handles,"releasedHandles":released_handles,
            "releasedPrivateBytes":after.private_usage,"peakPrivateBytes":after.peak_pagefile_usage,
            "peakWorkingSetBytes":after.peak_working_set,"outputHash":format!("{hash:016x}"),
            "processReadBytes":io_after.ReadTransferCount-io_before.ReadTransferCount,
            "processWriteBytes":io_after.WriteTransferCount-io_before.WriteTransferCount,
        })
    );
}
