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
    let sender = if legacy { &tx } else { &handle.tx };
    for offset in (0..seconds * 48_000).step_by(4096) {
        sender
            .send(AsrStreamInput::RawF32(vec![
                0.125;
                (seconds * 48_000 - offset)
                    .min(4096)
            ]))
            .unwrap();
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
