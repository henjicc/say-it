use super::*;
use crate::audio_dsp::{DspParams, StreamDsp};
use crate::performance_test_support::memory;
use std::time::Instant;

#[test]
#[ignore = "独立取消性能采样：合成积压和真实 DSP，不调用识别服务"]
fn cancel_backlog_profile() {
    let seconds = std::env::var("SAYIT_PERF_AUDIO_SECONDS")
        .unwrap_or("300".into())
        .parse::<usize>()
        .unwrap();
    assert!((1..=1800).contains(&seconds));
    let legacy = std::env::var("SAYIT_PERF_CANCEL_LEGACY").as_deref() == Ok("1");
    let initial = memory();
    let mut dsp = StreamDsp::new(DspParams::default(), 48_000);
    let (handle, priority) = AsrStreamHandle::channel();
    let (tx, mut fifo) = mpsc::unbounded_channel();
    for offset in (0..seconds * 48_000).step_by(4096) {
        let packet = AsrStreamInput::RawF32(vec![0.125; (seconds * 48_000 - offset).min(4096)]);
        if legacy {
            tx.send(packet).unwrap();
        } else {
            handle.tx.send(packet).unwrap();
        }
    }
    let queued = memory();
    let started = Instant::now();
    let mut next: Box<dyn FnMut() -> Option<AsrStreamInput>> = if legacy {
        tx.send(AsrStreamInput::Stop).unwrap();
        Box::new(move || fifo.blocking_recv())
    } else {
        handle.stop();
        let mut rx = priority;
        Box::new(move || rx.blocking_recv())
    };
    let mut item = next();
    let observed = memory();
    let mut processed = 0usize;
    loop {
        match item {
            Some(AsrStreamInput::RawF32(samples)) => {
                processed += samples.len();
                std::hint::black_box(dsp.process(&samples));
            }
            Some(AsrStreamInput::Stop) => break,
            _ => panic!("unexpected input"),
        }
        item = next();
    }
    let elapsed = started.elapsed();
    assert_eq!(processed, if legacy { seconds * 48_000 } else { 0 });
    drop(next);
    drop(dsp);
    let released = memory();
    println!(
        "PERF_RESULT {}",
        serde_json::json!({
            "scenario": "asr-cancel-backlog", "seconds": seconds, "legacy": legacy,
            "elapsedMs": elapsed.as_secs_f64() * 1000.0, "processedAfterCancel": processed,
            "initialPrivateBytes": initial.private_usage, "queuedPrivateBytes": queued.private_usage,
            "firstReceivePrivateBytes": observed.private_usage, "releasedPrivateBytes": released.private_usage,
            "peakPrivateBytes": released.peak_pagefile_usage, "peakWorkingSetBytes": released.peak_working_set,
        })
    );
}

#[test]
#[ignore = "独立慢消费者队列测量，仅合成输入和本地消费者"]
fn paced_backlog_profile() {
    let seconds = std::env::var("SAYIT_PERF_AUDIO_SECONDS")
        .unwrap_or("300".into())
        .parse::<usize>()
        .unwrap();
    assert!((1..=1800).contains(&seconds));
    let legacy = std::env::var("SAYIT_PERF_PACED_LEGACY").as_deref() == Ok("1");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let (handle, mut bounded) = AsrStreamHandle::channel();
    let (tx, mut unbounded) = mpsc::unbounded_channel();
    let mut receive: Box<dyn FnMut() -> Option<AsrStreamInput> + Send> = if legacy {
        Box::new(move || unbounded.blocking_recv())
    } else {
        Box::new(move || bounded.blocking_recv())
    };
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let waiting = barrier.clone();
    let consumer = std::thread::spawn(move || {
        waiting.wait();
        // 两边消费者同样先停顿 200ms；随后完整读取所有音频，不启动识别模型。
        std::thread::sleep(std::time::Duration::from_millis(200));
        let mut hash = 0xcbf29ce484222325_u64;
        let mut count = 0usize;
        loop {
            match receive() {
                Some(AsrStreamInput::RawF32(samples)) => {
                    count += samples.len();
                    for sample in samples {
                        hash = (hash ^ sample.to_bits() as u64).wrapping_mul(0x100000001b3);
                    }
                }
                Some(AsrStreamInput::Finish) => break,
                _ => panic!("输入意外结束"),
            }
        }
        (hash, count)
    });
    let initial = memory();
    let started = Instant::now();
    barrier.wait();
    let mut max_queued = 0;
    runtime.block_on(async {
        for offset in (0..seconds * 16_000).step_by(1600) {
            let packet = AsrStreamInput::RawF32(
                (offset..(offset + 1600).min(seconds * 16_000))
                    .map(|i| (i % 997) as f32 / 997.0 - 0.5)
                    .collect(),
            );
            if legacy {
                tx.send(packet).unwrap();
            } else {
                handle.tx.send_paced(packet).await.unwrap();
                let queued = handle.tx.budget.bytes.load(Ordering::Acquire);
                max_queued = max_queued.max(queued);
                assert!(queued <= PACED_QUEUE_BYTES);
            }
        }
    });
    let enqueued = started.elapsed();
    if legacy {
        tx.send(AsrStreamInput::Finish).unwrap();
    } else {
        handle.tx.send(AsrStreamInput::Finish).unwrap();
    }
    let (hash, count) = consumer.join().unwrap();
    let elapsed = started.elapsed();
    let after = memory();
    assert_eq!(count, seconds * 16_000);
    assert_eq!(handle.tx.budget.bytes.load(Ordering::Acquire), 0);
    println!(
        "PERF_RESULT {}",
        serde_json::json!({
            "scenario":"paced-asr-backlog", "legacy":legacy, "seconds":seconds,
            "consumerInitialDelayMs":200,"queueBudgetBytes":PACED_QUEUE_BYTES,
            "maxObservedQueuedBytes": if legacy { None } else { Some(max_queued) },
            "elapsedMs":elapsed.as_secs_f64()*1000.0, "enqueueMs":enqueued.as_secs_f64()*1000.0,
            "outputHash":format!("{hash:016x}"),"samples":count,
            "initialPrivateBytes":initial.private_usage,"releasedPrivateBytes":after.private_usage,
            "peakPrivateBytes":after.peak_pagefile_usage,"peakWorkingSetBytes":after.peak_working_set
        })
    );
}
