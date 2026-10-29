use super::*;
use crate::performance_test_support::{memory, thread_cycles};
use std::time::Instant;
use tokio::sync::broadcast;

fn exercise<T: Clone>(
    mut receivers: Vec<broadcast::Receiver<T>>,
    publish: impl Fn(BackendEvent),
    inspect: impl Fn(&T) -> &BackendEvent,
    count: usize,
    bytes: usize,
) -> usize {
    let mut delivered = 0;
    for start in (0..count).step_by(64) {
        for index in start..start + 64 {
            publish(BackendEvent::Asr {
                session_id: "s".into(),
                kind: "result".into(),
                payload: serde_json::json!({"sequence":index,"text":"x".repeat(bytes),"final":true}),
            });
        }
        // 三个真实广播订阅者同时持有这一批结果，模拟处理期间的重叠生命周期。
        let held: Vec<Vec<T>> = receivers
            .iter_mut()
            .map(|receiver| (0..64).map(|_| receiver.try_recv().unwrap()).collect())
            .collect();
        for events in &held {
            for (offset, event) in events.iter().enumerate() {
                let BackendEvent::Asr { payload, .. } = inspect(event) else {
                    panic!("wrong event")
                };
                assert_eq!(payload["sequence"], start + offset);
                assert_eq!(payload["final"], true);
                let text = payload["text"].as_str().unwrap();
                assert_eq!(text.len(), bytes);
                assert!(text.starts_with('x') && text.ends_with('x'));
                delivered += text.len();
            }
        }
    }
    delivered
}

#[test]
#[ignore = "独立后台事件扇出测量；不调用设备或服务"]
fn fanout_profile() {
    let legacy = std::env::var("SAYIT_PERF_EVENT_LEGACY").as_deref() == Ok("1");
    let small = std::env::var("SAYIT_PERF_EVENT_SMALL").as_deref() == Ok("1");
    let count = if small { 4096 } else { 64 };
    let bytes = if small { 128 } else { 256 * 1024 };
    let initial = memory();
    let cycles = thread_cycles();
    let started = Instant::now();
    let delivered = if legacy {
        // 冻结旧 Sender<BackendEvent>；广播对每个接收者深复制事件。
        let (sender, _) = broadcast::channel::<BackendEvent>(256);
        let receivers = (0..3).map(|_| sender.subscribe()).collect();
        exercise(
            receivers,
            |event| {
                sender.send(event).unwrap();
            },
            |event| event,
            count,
            bytes,
        )
    } else {
        let hub = BackendEventHub::default();
        let receivers = (0..3).map(|_| hub.subscribe()).collect();
        exercise(
            receivers,
            |event| hub.publish(event),
            |event| event.as_ref(),
            count,
            bytes,
        )
    };
    assert_eq!(delivered, count * bytes * 3);
    let elapsed = started.elapsed().as_secs_f64() * 1000.0;
    let cycles = thread_cycles() - cycles;
    let after = memory();
    println!(
        "PERF_RESULT {}",
        serde_json::json!({
            "scenario":"event-fanout", "legacy":legacy,"small":small,"events":count,
            "deliveredBytes":delivered,"elapsedMs":elapsed,"threadCycles":cycles,
            "initialPrivateBytes":initial.private_usage,"finalPrivateBytes":after.private_usage,
            "peakPrivateBytes":after.peak_pagefile_usage,
        })
    );
}
