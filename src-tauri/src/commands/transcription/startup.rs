use crate::cancellation::CancellationFlag;
use crate::prelude::*;
use crate::state::RuntimeState;
use std::sync::atomic::Ordering;

/// 工作启动前同时登记用途、取消入口和业务接收方，立即完成/失败也不能抢在归属登记之前。
pub(super) fn register_job(
    state: &RuntimeState,
    job_id: &str,
    kind: &str,
    cancel: Arc<CancellationFlag>,
    register_owner: impl FnOnce(&str) -> Result<(), String>,
) -> Result<(), String> {
    {
        let mut jobs = state
            .transcriptions
            .lock()
            .map_err(|_| "录音识别任务表锁定失败")?;
        state.transcription_runtime.register(job_id, kind)?;
        jobs.insert(job_id.into(), cancel.clone());
    }
    if let Err(error) = register_owner(job_id) {
        cancel.store(true, Ordering::Release);
        state
            .transcriptions
            .lock()
            .map_err(|_| "录音识别任务表锁定失败")?
            .remove(job_id);
        state.transcription_runtime.finish(job_id)?;
        return Err(error);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owner_registration_sees_cancellation_entry_before_any_job_event() {
        let state = RuntimeState::default();
        let cancel = Arc::new(CancellationFlag::new(false));
        let mut owner = None;
        register_job(&state, "job", "compare", cancel, |id| {
            assert!(state.transcriptions.lock().unwrap().contains_key(id));
            owner = Some(id.to_string());
            Ok(())
        })
        .unwrap();
        assert_eq!(owner.as_deref(), Some("job"));
        let event = state
            .transcription_runtime
            .apply_event("job", "completed", json!({}))
            .unwrap()
            .unwrap();
        assert_eq!(event["kind"], "compare");
    }

    #[test]
    fn rejected_owner_releases_registration_and_prevents_late_projection() {
        let state = RuntimeState::default();
        let cancel = Arc::new(CancellationFlag::new(false));
        assert_eq!(
            register_job(&state, "job", "compare", cancel.clone(), |_| Err(
                "已取消".into()
            ))
            .unwrap_err(),
            "已取消"
        );
        assert!(cancel.load(Ordering::Acquire));
        assert!(state.transcriptions.lock().unwrap().is_empty());
        assert!(state
            .transcription_runtime
            .apply_event("job", "completed", json!({}))
            .unwrap()
            .is_none());
        assert!(state.transcription_runtime.snapshots().is_empty());
    }
}
