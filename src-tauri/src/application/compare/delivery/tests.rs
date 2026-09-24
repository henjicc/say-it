use super::*;
use crate::application::events::{publish_backend_event, BackendEvent};
use futures_util::FutureExt;

fn configured() -> (RuntimeState, u64) {
    let state = RuntimeState::default();
    let epoch = state.compare_runtime.reset(vec![
        CompareCellSnapshot {
            index: 0,
            ..Default::default()
        },
        CompareCellSnapshot {
            index: 1,
            ..Default::default()
        },
    ]);
    {
        let mut compare = state.compare_runtime.inner.lock().unwrap();
        compare.sessions.insert("stream".into(), 0);
        compare.phase = "finalizing".into();
    }
    (state, epoch)
}

fn asr(state: &RuntimeState, kind: &str, payload: Value) {
    publish_backend_event(
        state,
        BackendEvent::Asr {
            session_id: "stream".into(),
            kind: kind.into(),
            payload,
        },
    );
}

#[test]
fn stalled_refresh_keeps_all_final_sentences_and_settles_without_broadcast_delivery() {
    let (state, _) = configured();
    let mut shared = state.backend_events.subscribe();
    let mut expected = String::new();
    for index in 0..1_000 {
        let text = format!("第{index}句。");
        asr(&state, "result", json!({"text":"临时", "final":false}));
        asr(&state, "result", json!({"text":text, "final":true}));
        expected.push_str(&text);
    }
    asr(&state, "ended", json!({}));
    let snapshot = state.compare_runtime.snapshot();
    assert_eq!(snapshot.cells[0].text, expected);
    assert!(
        snapshot.cells[0].committed.is_empty(),
        "快照不携带内部累计副本"
    );
    assert_eq!(snapshot.cells[0].status, "done");
    assert_eq!(snapshot.phase, "idle");
    assert!(state
        .compare_runtime
        .inner
        .lock()
        .unwrap()
        .sessions
        .is_empty());
    assert!(matches!(
        shared.try_recv(),
        Err(tokio::sync::broadcast::error::TryRecvError::Empty)
    ));
    assert!(state
        .compare_runtime
        .changed
        .notified()
        .now_or_never()
        .is_some());
    assert!(state
        .compare_runtime
        .changed
        .notified()
        .now_or_never()
        .is_none());
}

#[test]
fn error_and_end_keep_failure_and_reject_stale_stream_after_reset() {
    let (state, _) = configured();
    asr(&state, "error", json!({"message":"本地模拟失败"}));
    asr(&state, "ended", json!({}));
    assert_eq!(state.compare_runtime.snapshot().cells[0].status, "error");
    assert_eq!(
        state.compare_runtime.snapshot().cells[0].error_message,
        "本地模拟失败"
    );
    state.compare_runtime.reset(vec![CompareCellSnapshot {
        index: 0,
        ..Default::default()
    }]);
    assert!(!state
        .compare_runtime
        .record_asr_event("stream", "result", &json!({"text":"旧结果", "final":true}))
        .unwrap());
    assert!(state.compare_runtime.snapshot().cells[0].text.is_empty());
}

#[test]
fn file_completion_is_recorded_before_refresh_and_does_not_settle_while_exporting() {
    let (state, epoch) = configured();
    state
        .compare_runtime
        .register_file_job(epoch, "file", 1)
        .unwrap();
    state.compare_runtime.inner.lock().unwrap().preparing_file = true;
    asr(&state, "ended", json!({}));
    publish_backend_event(
        &state,
        BackendEvent::Transcription {
            job_id: "file".into(),
            stage: "completed".into(),
            payload: json!({"result":{"transcripts":[{"text":"第一行"},{"text":"第二行"}]}}),
        },
    );
    let snapshot = state.compare_runtime.snapshot();
    assert_eq!(snapshot.cells[1].text, "第一行\n第二行");
    assert_eq!(snapshot.cells[1].status, "done");
    assert_eq!(snapshot.phase, "finalizing");
    assert!(state.compare_runtime.inner.lock().unwrap().jobs.is_empty());
    assert!(!state
        .compare_runtime
        .record_file_event("file", "error", &json!({"message":"迟到"}))
        .unwrap());
    assert!(state.compare_runtime.finish_file_export(epoch));
    settle(&state);
    assert_eq!(state.compare_runtime.snapshot().phase, "idle");
}

#[test]
fn file_error_releases_job_and_cannot_register_for_an_old_epoch() {
    let (state, epoch) = configured();
    asr(&state, "ended", json!({}));
    state.compare_runtime.inner.lock().unwrap().phase = "finalizing".into();
    state
        .compare_runtime
        .register_file_job(epoch, "file", 1)
        .unwrap();
    assert!(state
        .compare_runtime
        .record_file_event("file", "error", &json!({"message":"取消"}))
        .unwrap());
    assert_eq!(state.compare_runtime.snapshot().phase, "idle");
    assert_eq!(
        state.compare_runtime.snapshot().cells[1].error_message,
        "取消"
    );
    state.compare_runtime.reset(vec![]);
    assert!(state
        .compare_runtime
        .register_file_job(epoch, "late", 1)
        .is_err());
    assert!(state.compare_runtime.inner.lock().unwrap().jobs.is_empty());
}

#[test]
fn unrelated_events_still_reach_dictation_and_subtitle_subscribers() {
    let (state, _) = configured();
    let mut first = state.backend_events.subscribe();
    let mut second = state.backend_events.subscribe();
    publish_backend_event(
        &state,
        BackendEvent::Asr {
            session_id: "dictation".into(),
            kind: "result".into(),
            payload: json!({"text":"完整", "final":true}),
        },
    );
    let a = first.try_recv().unwrap();
    let b = second.try_recv().unwrap();
    assert!(Arc::ptr_eq(&a, &b));
    assert!(state
        .compare_runtime
        .changed
        .notified()
        .now_or_never()
        .is_none());
}

#[tokio::test]
async fn updates_between_refresh_and_next_wait_are_not_lost() {
    let (state, _) = configured();
    asr(&state, "result", json!({"text":"部分", "final":false}));
    state.compare_runtime.changed.notified().await;
    asr(&state, "result", json!({"text":"最终", "final":true}));
    asr(&state, "ended", json!({}));
    tokio::time::timeout(
        std::time::Duration::from_secs(1),
        state.compare_runtime.changed.notified(),
    )
    .await
    .unwrap();
    assert_eq!(state.compare_runtime.snapshot().cells[0].text, "最终");
}

#[cfg(windows)]
#[test]
#[ignore = "独立比较结果交付缓冲测试，不调用模型或设备"]
fn stalled_comparison_profile() {
    use crate::performance_test_support::{memory, thread_cycles};
    let legacy = std::env::var("SAYIT_PERF_COMPARISON_DELIVERY_LEGACY").as_deref() == Ok("1");
    let (state, _) = configured();
    // 冻结旧交付容量与共享载荷，不计算真实模型和界面序列化。
    let (sender, mut receiver) = tokio::sync::broadcast::channel::<Arc<BackendEvent>>(256);
    let text = "识别🙂".repeat(6_554);
    let initial = memory();
    let cycles = thread_cycles();
    let started = std::time::Instant::now();
    for index in 0..255 {
        let event = BackendEvent::Asr {
            session_id: "stream".into(),
            kind: "result".into(),
            payload: json!({"text":text,"final":index == 254}),
        };
        if legacy {
            sender.send(Arc::new(event)).unwrap();
        } else {
            publish_backend_event(&state, event);
        }
    }
    let ended = BackendEvent::Asr {
        session_id: "stream".into(),
        kind: "ended".into(),
        payload: json!({}),
    };
    if legacy {
        sender.send(Arc::new(ended)).unwrap();
    } else {
        publish_backend_event(&state, ended);
    }
    let stalled = memory();
    if legacy {
        for _ in 0..256 {
            let event = receiver.try_recv().unwrap();
            if let BackendEvent::Asr {
                session_id,
                kind,
                payload,
            } = event.as_ref()
            {
                state
                    .compare_runtime
                    .record_asr_event(session_id, kind, payload)
                    .unwrap();
            } else {
                panic!("unexpected event");
            }
        }
    }
    let snapshot = state.compare_runtime.snapshot();
    assert_eq!(snapshot.cells[0].text, text);
    assert_eq!(snapshot.phase, "idle");
    let elapsed = started.elapsed().as_secs_f64() * 1_000.0;
    let after = memory();
    let hash = text.bytes().fold(0xcbf29ce484222325u64, |hash, byte| {
        (hash ^ byte as u64).wrapping_mul(0x100000001b3)
    });
    println!(
        "PERF_RESULT {}",
        json!({
            "scenario":"comparison-delivery", "legacy":legacy, "updates":256, "textBytes":text.len(),
            "elapsedMs":elapsed, "threadCycles":thread_cycles()-cycles, "outputHash":format!("{hash:016x}"),
            "initialPrivateBytes":initial.private_usage, "stalledPrivateBytes":stalled.private_usage,
            "peakPrivateBytes":after.peak_pagefile_usage,
        })
    );
}
