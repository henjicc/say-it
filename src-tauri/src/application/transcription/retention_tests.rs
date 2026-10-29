use super::*;
use serde_json::json;

fn complete(runtime: &TranscriptionRuntime, id: &str, kind: &str) -> Value {
    runtime.register(id, kind).unwrap();
    let result = runtime
        .apply_event(
            id,
            "completed",
            json!({"result":{"transcripts":[{"text":id}]}}),
        )
        .unwrap()
        .unwrap();
    runtime.finish(id).unwrap();
    result
}

#[test]
fn repeated_jobs_keep_only_latest_page_results_and_no_completed_registrations() {
    let runtime = TranscriptionRuntime::default();
    for index in 0..1000 {
        for kind in TRANSCRIPTION_JOB_KINDS {
            let id = format!("{kind}-{index}");
            let result = complete(&runtime, &id, kind);
            assert_eq!(result["kind"], *kind);
            assert_eq!(result["result"]["transcripts"][0]["text"], id);
        }
    }
    let snapshots = runtime.snapshots();
    assert_eq!(snapshots.len(), 2);
    assert!(snapshots
        .iter()
        .all(|job| job.job_id.ends_with("-999") && !job.active));
    let inner = runtime.inner.lock().unwrap();
    assert!(inner.registered.is_empty());
    assert_eq!(inner.latest.len(), 2);
}

#[test]
fn older_active_job_can_finish_without_replacing_the_newer_result() {
    let runtime = TranscriptionRuntime::default();
    runtime.register("z-older", "transcribe").unwrap();
    runtime
        .apply_event("z-older", "uploading", json!({}))
        .unwrap();
    complete(&runtime, "a-newer", "transcribe");
    assert_eq!(runtime.domain_snapshot().state, DomainRunState::Running);
    assert_eq!(runtime.snapshots().len(), 2);
    let delivered = runtime
        .apply_event("z-older", "completed", json!({"result":"older"}))
        .unwrap()
        .unwrap();
    assert_eq!(
        delivered["result"], "older",
        "仍须向已有订阅方完整发送旧任务结果"
    );
    runtime.finish("z-older").unwrap();
    assert_eq!(runtime.domain_snapshot().state, DomainRunState::Idle);
    assert_eq!(runtime.snapshots()[0].job_id, "a-newer");
}

#[test]
fn cancellation_keeps_registration_until_worker_exits_and_late_events_cannot_recreate_it() {
    let runtime = TranscriptionRuntime::default();
    runtime.register("compare-1", "compare").unwrap();
    let event = runtime
        .apply_event("compare-1", "error", json!({"cancelled":true}))
        .unwrap()
        .unwrap();
    assert_eq!(event["kind"], "compare");
    assert!(runtime.snapshots().is_empty());
    assert!(runtime
        .inner
        .lock()
        .unwrap()
        .registered
        .contains_key("compare-1"));
    assert_eq!(
        runtime
            .apply_event("compare-1", "error", json!({}))
            .unwrap()
            .unwrap()["kind"],
        "compare"
    );
    runtime.finish("compare-1").unwrap();
    assert!(runtime
        .apply_event("compare-1", "error", json!({}))
        .unwrap()
        .is_none());
    assert!(runtime.snapshots().is_empty());
}

#[test]
fn starting_a_new_page_job_releases_the_replaced_result_and_preserves_other_page() {
    let runtime = TranscriptionRuntime::default();
    complete(&runtime, "old-transcribe", "transcribe");
    complete(&runtime, "old-align", "align");
    runtime.register("new-transcribe", "transcribe").unwrap();
    assert!(runtime.get("old-transcribe").is_none());
    assert!(runtime.get("old-align").is_some());
    runtime
        .apply_event("new-transcribe", "error", json!({"message":"failed"}))
        .unwrap();
    runtime.finish("new-transcribe").unwrap();
    assert_eq!(
        runtime.get("new-transcribe").unwrap().payload["message"],
        "failed"
    );
    assert_eq!(runtime.snapshots().len(), 2);
}

#[cfg(windows)]
#[test]
#[ignore = "独立重复转写结果保留测量；不访问音频设备或服务"]
fn repeated_result_profile() {
    use crate::performance_test_support::memory;
    use std::time::Instant;
    let legacy = std::env::var("SAYIT_PERF_TRANSCRIPTION_LEGACY").as_deref() == Ok("1");
    let runtime = TranscriptionRuntime::default();
    // 冻结旧投影的完整结果与用途表；序列化结果形状与新路径相同。
    let mut old_jobs = HashMap::<String, TranscriptionJobSnapshot>::new();
    let mut old_kinds = HashMap::<String, String>::new();
    let initial = memory();
    let start = Instant::now();
    let mut delivered_bytes = 0;
    for index in 0..256 {
        let kind = TRANSCRIPTION_JOB_KINDS[index % 4];
        let id = format!("job-{index}");
        let text = format!("{index:08}{}", "识别结果。".repeat(32_768));
        let payload = json!({"jobId":id,"kind":kind,"stage":"completed","result":{"transcripts":[{"text":text}]}});
        if legacy {
            old_kinds.insert(id.clone(), kind.into());
            old_jobs.insert(
                id.clone(),
                TranscriptionJobSnapshot {
                    job_id: id,
                    kind: kind.into(),
                    stage: "completed".into(),
                    active: false,
                    payload: payload.clone(),
                },
            );
            delivered_bytes += payload["result"]["transcripts"][0]["text"]
                .as_str()
                .unwrap()
                .len();
        } else {
            runtime.register(&id, kind).unwrap();
            let delivered = runtime
                .apply_event(&id, "completed", payload)
                .unwrap()
                .unwrap();
            delivered_bytes += delivered["result"]["transcripts"][0]["text"]
                .as_str()
                .unwrap()
                .len();
            runtime.finish(&id).unwrap();
        }
    }
    assert_eq!(delivered_bytes, 256 * (8 + "识别结果。".len() * 32_768));
    let retained = if legacy {
        old_jobs.len()
    } else {
        runtime.snapshots().len()
    };
    assert_eq!(retained, if legacy { 256 } else { 2 });
    let after = memory();
    println!(
        "PERF_RESULT {}",
        json!({"scenario":"transcription-retention", "legacy":legacy,
        "elapsedMs":start.elapsed().as_secs_f64()*1000.0, "deliveredBytes":delivered_bytes,
        "retainedJobs":retained,"initialPrivateBytes":initial.private_usage,
        "finalPrivateBytes":after.private_usage, "peakPrivateBytes":after.peak_pagefile_usage})
    );
}
