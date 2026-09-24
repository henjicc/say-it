use super::*;

#[test]
fn release_bursts_keep_one_worker_and_wait_for_last_release() {
    let start = Instant::now();
    let mut plan = Plan::default();
    assert!(plan.request(start));
    for second in 1..20 {
        assert!(!plan.request(start + Duration::from_secs(second)));
    }
    assert!(!plan.claim(start + Duration::from_secs(23)));
    assert!(plan.claim(start + Duration::from_secs(24)));
    assert!(!plan.claim(start + Duration::from_secs(25)));
    assert!(plan.next_deadline().is_none());
    assert!(plan.request(start + Duration::from_secs(30)));
}

#[test]
fn release_during_reclaim_is_handled_by_existing_worker() {
    let start = Instant::now();
    let mut plan = Plan::default();
    assert!(plan.request(start));
    assert!(plan.claim(start + IDLE_DELAY));
    assert!(!plan.request(start + IDLE_DELAY));
    assert_eq!(plan.next_deadline(), Some(start + IDLE_DELAY * 2));
    assert!(plan.claim(start + IDLE_DELAY * 2));
    assert_eq!(plan.next_deadline(), None);
    assert!(!plan.worker_running);
}

#[test]
fn no_release_event_means_no_timer_and_no_reclaim() {
    let mut plan = Plan::default();
    assert!(plan.next_deadline().is_none());
    assert!(!plan.claim(Instant::now() + Duration::from_secs(3600)));
    assert!(!plan.worker_running);
}

#[test]
fn running_audio_recognition_and_pending_jobs_prevent_reclaim() {
    use crate::application::audio_session::AudioOwner;
    use crate::state::AsrStreamHandle;
    let state = RuntimeState::default();
    assert!(eligible(&state));
    let lease = state.audio_session.acquire(AudioOwner::Dictation).unwrap();
    assert!(!eligible(&state));
    state.audio_session.release(&lease).unwrap();
    let (handle, _rx) = AsrStreamHandle::channel();
    state
        .asr_streams
        .lock()
        .unwrap()
        .insert("test".into(), handle);
    assert!(!eligible(&state));
    state.asr_streams.lock().unwrap().clear();
    state.transcriptions.lock().unwrap().insert(
        "test".into(),
        std::sync::Arc::new(crate::cancellation::CancellationFlag::new(false)),
    );
    assert!(!eligible(&state));
    state.transcriptions.lock().unwrap().clear();
    let lease = state.audio_session.acquire(AudioOwner::Comparison).unwrap();
    assert!(!eligible(&state));
    state.audio_session.release(&lease).unwrap();
    assert!(eligible(&state));
}

#[test]
fn recording_audio_lab_prevents_reclaim() {
    let state = RuntimeState::default();
    assert!(eligible(&state));
    state.audio_lab_runtime.begin(48_000).unwrap();
    assert!(!eligible(&state));
}
