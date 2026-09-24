use super::*;
use futures_util::FutureExt;

fn runtime() -> SubtitleRuntime {
    let runtime = SubtitleRuntime::default();
    {
        let mut session = runtime.session.lock().unwrap();
        session.epoch = 7;
        session.phase = SubtitlePhase::Running;
        session.prefs.translation_model = "test-model".into();
    }
    runtime
}

fn segment(runtime: &SubtitleRuntime) -> u64 {
    let mut session = runtime.session.lock().unwrap();
    let seq = session.translation.dispatch("一句", true)[0].0;
    session.translation.commit("replace", true);
    seq
}

fn consume_one_notification(runtime: &SubtitleRuntime) {
    assert!(runtime
        .translation_changed
        .notified()
        .now_or_never()
        .is_some());
    assert!(runtime
        .translation_changed
        .notified()
        .now_or_never()
        .is_none());
}

#[test]
fn final_is_committed_even_when_the_renderer_and_shared_broadcast_lag() {
    let runtime = runtime();
    let seq = segment(&runtime);
    let hub = crate::application::events::BackendEventHub::default();
    let mut receiver = hub.subscribe();
    for index in 0..1_000 {
        assert!(runtime
            .record_translation(7, seq, &format!("临时{index}"), false, None)
            .unwrap());
    }
    assert!(runtime
        .record_translation(7, seq, "最终译文", true, None)
        .unwrap());
    for _ in 0..1_000 {
        hub.publish(BackendEvent::Asr {
            session_id: "unrelated".into(),
            kind: "result".into(),
            payload: serde_json::json!({"text":"其他识别"}),
        });
    }
    assert!(matches!(
        receiver.try_recv(),
        Err(tokio::sync::broadcast::error::TryRecvError::Lagged(_))
    ));
    let session = runtime.session.lock().unwrap();
    assert_eq!(session.translation.display(&session.prefs), "最终译文");
    assert!(session.translation.completed.contains(&seq));
    drop(session);
    consume_one_notification(&runtime);
    assert!(!runtime
        .record_translation(7, seq, "迟到临时", false, None)
        .unwrap());
    assert!(runtime
        .translation_changed
        .notified()
        .now_or_never()
        .is_none());
}

#[test]
fn out_of_order_short_final_preserves_previous_text() {
    let runtime = runtime();
    let first = segment(&runtime);
    let second = segment(&runtime);
    runtime
        .record_translation(7, second, &"临时🙂".repeat(20_000), false, None)
        .unwrap();
    assert_eq!(
        runtime.session.lock().unwrap().translation.values[&second]
            .chars()
            .count(),
        MAX_TEXT_CHARS + 1
    );
    runtime
        .record_translation(7, first, "第一句", true, None)
        .unwrap();
    runtime
        .record_translation(7, second, "短最终", true, None)
        .unwrap();
    let session = runtime.session.lock().unwrap();
    assert_eq!(session.translation.display(&session.prefs), "第一句 短最终");
    assert_eq!(session.translation.completed.len(), 2);
}

#[test]
fn failure_retains_partial_and_settles_the_segment() {
    let runtime = runtime();
    let seq = segment(&runtime);
    runtime
        .record_translation(7, seq, "已有部分", false, None)
        .unwrap();
    runtime
        .record_translation(7, seq, "", true, Some("本地模拟失败"))
        .unwrap();
    let session = runtime.session.lock().unwrap();
    assert_eq!(session.translation.values[&seq], "已有部分");
    assert!(session.translation.completed.contains(&seq));
    assert_eq!(
        session.translation_error.as_deref(),
        Some("字幕翻译失败：本地模拟失败")
    );
    drop(session);
    consume_one_notification(&runtime);
}

#[test]
fn stale_unknown_and_stopped_results_do_not_mutate_or_notify() {
    let runtime = runtime();
    let seq = segment(&runtime);
    assert!(!runtime
        .record_translation(6, seq, "旧会话", true, None)
        .unwrap());
    assert!(!runtime
        .record_translation(7, seq + 1, "未知序号", true, None)
        .unwrap());
    for phase in [SubtitlePhase::Stopping, SubtitlePhase::Idle] {
        runtime.session.lock().unwrap().phase = phase;
        assert!(!runtime
            .record_translation(7, seq, "停止后结果", true, None)
            .unwrap());
    }
    let session = runtime.session.lock().unwrap();
    assert!(session.translation.values[&seq].is_empty());
    assert!(session.translation.completed.is_empty());
    assert!(runtime
        .translation_changed
        .notified()
        .now_or_never()
        .is_none());
}

#[test]
fn replacement_with_a_pending_notification_cannot_restore_old_payload() {
    let runtime = runtime();
    let seq = segment(&runtime);
    runtime
        .record_translation(7, seq, "旧结果", true, None)
        .unwrap();
    *runtime.session.lock().unwrap() = Session {
        epoch: 8,
        ..Session::default()
    };
    assert!(!runtime
        .record_translation(7, seq, "迟到", true, None)
        .unwrap());
    consume_one_notification(&runtime);
    let session = runtime.session.lock().unwrap();
    assert_eq!(session.epoch, 8);
    assert!(session.translation.display(&session.prefs).is_empty());
}

#[test]
fn stalled_rendering_does_not_retain_completed_history_or_empty_placeholders() {
    let runtime = runtime();
    for index in 0..4_000 {
        let seq = segment(&runtime);
        let text = if index % 2 == 0 {
            "译".repeat(300)
        } else {
            String::new()
        };
        runtime
            .record_translation(7, seq, &text, true, None)
            .unwrap();
        let session = runtime.session.lock().unwrap();
        assert!(session.translation.values.len() <= 12);
        assert!(session.translation.completed.len() <= 12);
    }
    consume_one_notification(&runtime);
}

#[tokio::test]
async fn update_during_refresh_remains_wakeable() {
    let runtime = runtime();
    let seq = segment(&runtime);
    runtime
        .record_translation(7, seq, "临时", false, None)
        .unwrap();
    runtime.translation_changed.notified().await;
    // 渲染已取走通知，但下一次等待还未注册。
    runtime
        .record_translation(7, seq, "最终", true, None)
        .unwrap();
    tokio::time::timeout(
        Duration::from_secs(1),
        runtime.translation_changed.notified(),
    )
    .await
    .unwrap();
    assert!(runtime
        .session
        .lock()
        .unwrap()
        .translation
        .completed
        .contains(&seq));
}

#[cfg(windows)]
#[test]
#[ignore = "独立测量显示消费者暂停时的译文载荷；不调用服务"]
fn stalled_renderer_profile() {
    use crate::performance_test_support::{memory, thread_cycles};
    let legacy = std::env::var("SAYIT_PERF_TRANSLATION_DELIVERY_LEGACY").as_deref() == Ok("1");
    let runtime = runtime();
    let seq = segment(&runtime);
    // 冻结旧实现的容量与载荷所有权；只重现翻译交付，不包含供应商网络开销。
    let (sender, mut receiver) = tokio::sync::broadcast::channel::<Arc<(String, bool)>>(256);
    let partial = "译文🙂".repeat(6_554);
    let initial = memory();
    let cycles = thread_cycles();
    let started = Instant::now();
    for index in 0..512 {
        let done = index == 511;
        let text = if done { "最终完整译文" } else { &partial };
        if legacy {
            sender.send(Arc::new((text.to_owned(), done))).unwrap();
        } else {
            runtime
                .record_translation(7, seq, text, done, None)
                .unwrap();
        }
    }
    let stalled = memory();
    let mut delivered = 0;
    if legacy {
        loop {
            match receiver.try_recv() {
                Ok(value) => {
                    runtime
                        .record_translation(7, seq, &value.0, value.1, None)
                        .unwrap();
                    delivered += 1;
                }
                Err(tokio::sync::broadcast::error::TryRecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::TryRecvError::Empty) => break,
                Err(error) => panic!("unexpected delivery error: {error}"),
            }
        }
    }
    let session = runtime.session.lock().unwrap();
    assert_eq!(session.translation.display(&session.prefs), "最终完整译文");
    assert!(session.translation.completed.contains(&seq));
    let elapsed = started.elapsed().as_secs_f64() * 1_000.0;
    let after = memory();
    println!(
        "PERF_RESULT {}",
        serde_json::json!({
            "scenario":"subtitle-delivery", "legacy":legacy, "updates":512,
            "partialBytes":partial.len(), "legacyDelivered":delivered,
            "elapsedMs":elapsed, "threadCycles":thread_cycles()-cycles,
            "initialPrivateBytes":initial.private_usage, "stalledPrivateBytes":stalled.private_usage,
            "peakPrivateBytes":after.peak_pagefile_usage,
            "output":"最终完整译文",
        })
    );
}
