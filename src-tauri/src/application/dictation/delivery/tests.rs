use super::*;
use crate::application::events::{publish_backend_event, BackendEvent};
use futures_util::FutureExt;

fn configured(phase: DictationPhase) -> RuntimeState {
    let state = RuntimeState::default();
    *state.dictation_runtime.session.lock().unwrap() = Session {
        epoch: 7,
        phase,
        asr_session_id: Some("stream".into()),
        ..Default::default()
    };
    state
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
fn stalled_consumer_keeps_every_final_and_only_one_completion_action() {
    let state = configured(DictationPhase::Finishing);
    let mut broadcast = state.backend_events.subscribe();
    let mut expected = String::new();
    for index in 0..1_000 {
        let text = format!("第{index}句。");
        asr(&state, "result", json!({"text":"临时", "final":false}));
        asr(&state, "result", json!({"text":text, "final":true}));
        expected.push_str(&text);
    }
    asr(&state, "finish", json!({}));
    asr(&state, "ended", json!({}));
    asr(&state, "error", json!({"message":"结束后的迟到错误"}));
    asr(&state, "result", json!({"text":"迟到数据", "final":true}));
    assert_eq!(
        state.dictation_runtime.session.lock().unwrap().committed,
        expected
    );
    assert_eq!(
        state.dictation_runtime.take_pending().unwrap(),
        Some((7, Outcome::Finalize))
    );
    assert_eq!(state.dictation_runtime.take_pending().unwrap(), None);
    assert!(state
        .dictation_runtime
        .changed
        .notified()
        .now_or_never()
        .is_some());
    assert!(state
        .dictation_runtime
        .changed
        .notified()
        .now_or_never()
        .is_none());
    assert!(matches!(
        broadcast.try_recv(),
        Err(tokio::sync::broadcast::error::TryRecvError::Empty)
    ));
}

#[test]
fn failure_is_latched_and_timeout_cannot_claim_injection_before_or_after_dispatch() {
    let state = configured(DictationPhase::Finishing);
    asr(&state, "result", json!({"text":"已识别原文", "final":true}));
    asr(&state, "error", json!({"message":"断网"}));
    asr(&state, "finish_timeout", json!({}));
    assert!(!state
        .dictation_runtime
        .session
        .lock()
        .unwrap()
        .claim_injection(7));
    assert_eq!(
        state.dictation_runtime.take_pending().unwrap(),
        Some((7, Outcome::Fail("实时语音识别失败：断网".into())))
    );
    let mut session = state.dictation_runtime.session.lock().unwrap();
    assert!(!session.claim_injection(7));
    assert_eq!(session.committed, "已识别原文");
    assert_eq!(
        session.asr_session_id.as_deref(),
        Some("stream"),
        "失败清理仍能拿到需要停止的流"
    );
}

#[test]
fn completion_claim_is_once_and_late_file_events_cannot_change_frozen_text() {
    let state = configured(DictationPhase::ProcessingFile);
    state.dictation_runtime.session.lock().unwrap().file_job_id = Some("file".into());
    publish_backend_event(
        &state,
        BackendEvent::Transcription {
            job_id: "file".into(),
            stage: "completed".into(),
            payload: json!({"result":{"transcripts":[{"text":"第一行"},{"text":"第二行"}]}}),
        },
    );
    assert_eq!(
        state.dictation_runtime.take_pending().unwrap(),
        Some((7, Outcome::Finalize))
    );
    {
        let mut session = state.dictation_runtime.session.lock().unwrap();
        assert!(session.claim_injection(7));
        assert!(!session.claim_injection(7));
        session.phase = DictationPhase::Injecting;
    }
    assert!(state
        .dictation_runtime
        .record_file_event(
            "file",
            "completed",
            &json!({"result":{"transcripts":[{"text":"迟到"}]}})
        )
        .unwrap());
    assert!(state
        .dictation_runtime
        .record_file_event("file", "error", &json!({"message":"迟到错误"}))
        .unwrap());
    assert_eq!(
        state.dictation_runtime.session.lock().unwrap().committed,
        "第一行\n第二行"
    );
    assert_eq!(state.dictation_runtime.take_pending().unwrap(), None);
}

#[test]
fn file_failure_blocks_finalize_and_preserves_previous_text() {
    let state = configured(DictationPhase::ProcessingFile);
    {
        let mut session = state.dictation_runtime.session.lock().unwrap();
        session.file_job_id = Some("file".into());
        session.committed = "原文".into();
    }
    state
        .dictation_runtime
        .record_file_event("file", "error", &json!({"message":"服务失败"}))
        .unwrap();
    assert_eq!(
        state.dictation_runtime.take_pending().unwrap(),
        Some((7, Outcome::Fail("服务失败".into())))
    );
    let mut session = state.dictation_runtime.session.lock().unwrap();
    assert!(!session.claim_injection(7));
    assert_eq!(session.committed, "原文");
}

#[test]
fn unexpected_close_is_failure_but_normal_finish_during_recording_is_not_a_terminal() {
    let state = configured(DictationPhase::Recording);
    asr(&state, "finish", json!({}));
    assert_eq!(state.dictation_runtime.take_pending().unwrap(), None);
    asr(&state, "result", json!({"text":"最后部分", "final":false}));
    asr(&state, "closed", json!({}));
    assert_eq!(
        state.dictation_runtime.take_pending().unwrap(),
        Some((7, Outcome::Fail("实时语音识别连接意外中断".into())))
    );
    assert_eq!(
        state.dictation_runtime.session.lock().unwrap().segment,
        "最后部分"
    );
}

#[test]
fn terminal_states_and_old_connections_cannot_modify_or_schedule_new_output() {
    for phase in [
        DictationPhase::Idle,
        DictationPhase::Failed,
        DictationPhase::Injecting,
    ] {
        let state = configured(phase);
        asr(&state, "result", json!({"text":"迟到", "final":true}));
        asr(&state, "error", json!({"message":"迟到失败"}));
        assert!(state
            .dictation_runtime
            .session
            .lock()
            .unwrap()
            .committed
            .is_empty());
        assert_eq!(state.dictation_runtime.take_pending().unwrap(), None);
        assert!(state
            .dictation_runtime
            .changed
            .notified()
            .now_or_never()
            .is_none());
    }
    let state = configured(DictationPhase::Recording);
    assert!(!state
        .dictation_runtime
        .record_asr_event("old", "result", &json!({"text":"旧连接"}))
        .unwrap());
    assert!(!state
        .dictation_runtime
        .record_file_event("unknown", "completed", &json!({}))
        .unwrap());
}

#[tokio::test]
async fn replacing_a_session_while_old_processing_waits_keeps_new_data_and_completion() {
    let state = configured(DictationPhase::Finishing);
    asr(&state, "finish", json!({}));
    state.dictation_runtime.changed.notified().await;
    let (old_epoch, _) = state.dictation_runtime.take_pending().unwrap().unwrap();
    // 模拟旧后处理正在 await；新会话替换后，生产者不依赖旧消费者恢复才提交数据。
    *state.dictation_runtime.session.lock().unwrap() = Session {
        epoch: 8,
        phase: DictationPhase::Finishing,
        asr_session_id: Some("new".into()),
        ..Default::default()
    };
    for _ in 0..1_000 {
        state
            .dictation_runtime
            .record_asr_event("new", "result", &json!({"text":"新", "final":true}))
            .unwrap();
    }
    state
        .dictation_runtime
        .record_asr_event("new", "finish", &json!({}))
        .unwrap();
    assert!(!state
        .dictation_runtime
        .session
        .lock()
        .unwrap()
        .claim_injection(old_epoch));
    tokio::time::timeout(
        Duration::from_secs(1),
        state.dictation_runtime.changed.notified(),
    )
    .await
    .unwrap();
    assert_eq!(
        state.dictation_runtime.session.lock().unwrap().committed,
        "新".repeat(1_000)
    );
    assert_eq!(
        state.dictation_runtime.take_pending().unwrap(),
        Some((8, Outcome::Finalize))
    );
}

#[cfg(windows)]
#[test]
#[ignore = "独立听写交付缓冲测试，不调用设备、识别或文字输出"]
fn stalled_dictation_profile() {
    use crate::performance_test_support::{memory, thread_cycles};
    let legacy = std::env::var("SAYIT_PERF_DICTATION_DELIVERY_LEGACY").as_deref() == Ok("1");
    let small = std::env::var("SAYIT_PERF_DICTATION_DELIVERY_SMALL").as_deref() == Ok("1");
    let state = configured(DictationPhase::Finishing);
    let (sender, mut receiver) = tokio::sync::broadcast::channel::<Arc<BackendEvent>>(256);
    let text = if small {
        "句".repeat(42)
    } else {
        "听写🙂".repeat(6_554)
    };
    let initial = memory();
    let cycles = thread_cycles();
    let started = Instant::now();
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
    let finished = BackendEvent::Asr {
        session_id: "stream".into(),
        kind: "finish".into(),
        payload: json!({}),
    };
    if legacy {
        sender.send(Arc::new(finished)).unwrap();
    } else {
        publish_backend_event(&state, finished);
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
                    .dictation_runtime
                    .record_asr_event(session_id, kind, payload)
                    .unwrap();
            } else {
                panic!("unexpected event");
            }
        }
    }
    assert_eq!(
        state.dictation_runtime.take_pending().unwrap(),
        Some((7, Outcome::Finalize))
    );
    assert_eq!(
        state.dictation_runtime.session.lock().unwrap().committed,
        text
    );
    let elapsed = started.elapsed().as_secs_f64() * 1_000.0;
    let after = memory();
    let hash = text.bytes().fold(0xcbf29ce484222325u64, |hash, byte| {
        (hash ^ byte as u64).wrapping_mul(0x100000001b3)
    });
    println!(
        "PERF_RESULT {}",
        json!({
            "scenario":"dictation-delivery", "legacy":legacy, "small":small,"updates":256,"textBytes":text.len(),
            "elapsedMs":elapsed,"threadCycles":thread_cycles()-cycles,"outputHash":format!("{hash:016x}"),
            "initialPrivateBytes":initial.private_usage,"stalledPrivateBytes":stalled.private_usage,
            "peakPrivateBytes":after.peak_pagefile_usage,
        })
    );
}
