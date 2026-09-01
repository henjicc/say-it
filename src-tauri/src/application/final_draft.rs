use serde_json::json;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};
use tokio::sync::mpsc;

use crate::active_app_context::{ActivationTarget, AppIdentity};

const MAX_DRAFT_CHARS: usize = 6000;
const OBSERVATION_TIMEOUT: Duration = Duration::from_secs(120);
const KEYBOARD_CLEAR_WINDOW: Duration = Duration::from_secs(2);
const CLICK_CLEAR_WINDOW: Duration = Duration::from_secs(3);

#[derive(Clone, Debug, PartialEq, Eq)]
struct DraftSnapshot {
    value: String,
    selection_location: usize,
    selection_length: usize,
    truncated: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ObserverEventKind {
    ValueChanged,
    FocusChanged,
    ReturnPressed,
    Clicked,
}

#[derive(Clone, Debug)]
struct ObserverEvent {
    kind: ObserverEventKind,
    value: Option<String>,
}

trait FinalDraftObserverPort {
    fn start(
        process_id: u32,
        max_chars: usize,
    ) -> Result<
        (
            PlatformObserver,
            DraftSnapshot,
            mpsc::UnboundedReceiver<ObserverEvent>,
        ),
        String,
    >;
}

#[cfg(target_os = "macos")]
struct PlatformDraftObserverPort;

#[cfg(target_os = "macos")]
impl FinalDraftObserverPort for PlatformDraftObserverPort {
    fn start(
        process_id: u32,
        max_chars: usize,
    ) -> Result<
        (
            PlatformObserver,
            DraftSnapshot,
            mpsc::UnboundedReceiver<ObserverEvent>,
        ),
        String,
    > {
        let (observer, snapshot, mut native_events) =
            crate::macos_native::start_final_draft_observer(process_id, max_chars as u32)?;
        let (sender, receiver) = mpsc::unbounded_channel();
        tauri::async_runtime::spawn(async move {
            while let Some(event) = native_events.recv().await {
                let kind = match event.kind {
                    crate::macos_native::MacFinalDraftEventKind::ValueChanged => {
                        ObserverEventKind::ValueChanged
                    }
                    crate::macos_native::MacFinalDraftEventKind::FocusChanged => {
                        ObserverEventKind::FocusChanged
                    }
                    crate::macos_native::MacFinalDraftEventKind::ReturnPressed => {
                        ObserverEventKind::ReturnPressed
                    }
                    crate::macos_native::MacFinalDraftEventKind::Clicked => {
                        ObserverEventKind::Clicked
                    }
                };
                if sender
                    .send(ObserverEvent {
                        kind,
                        value: event.value,
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        Ok((
            PlatformObserver::Mac(observer),
            DraftSnapshot {
                value: snapshot.value,
                selection_location: snapshot.selection_location,
                selection_length: snapshot.selection_length,
                truncated: snapshot.truncated,
            },
            receiver,
        ))
    }
}

#[cfg(not(target_os = "macos"))]
struct PlatformDraftObserverPort;

#[cfg(not(target_os = "macos"))]
impl FinalDraftObserverPort for PlatformDraftObserverPort {
    fn start(
        _process_id: u32,
        _max_chars: usize,
    ) -> Result<
        (
            PlatformObserver,
            DraftSnapshot,
            mpsc::UnboundedReceiver<ObserverEvent>,
        ),
        String,
    > {
        Err("unsupported: 当前平台尚未实现最终草稿观察".into())
    }
}

enum PlatformObserver {
    #[cfg(target_os = "macos")]
    Mac(crate::macos_native::MacFinalDraftObserver),
    #[cfg(not(target_os = "macos"))]
    Unsupported,
}

impl PlatformObserver {
    fn snapshot(&self) -> Result<DraftSnapshot, String> {
        #[cfg(target_os = "macos")]
        match self {
            Self::Mac(observer) => {
                let snapshot = observer.snapshot()?;
                Ok(DraftSnapshot {
                    value: snapshot.value,
                    selection_location: snapshot.selection_location,
                    selection_length: snapshot.selection_length,
                    truncated: snapshot.truncated,
                })
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = self;
            Err("unsupported: 当前平台尚未实现最终草稿观察".into())
        }
    }
}

struct SendCandidate {
    at: Instant,
    clear_window: Duration,
    confidence: &'static str,
    source: &'static str,
    value: String,
    revision: u64,
}

#[cfg(test)]
fn confirmed_candidate(candidate: Option<&SendCandidate>) -> Option<(&'static str, &'static str)> {
    candidate
        .filter(|candidate| candidate.at.elapsed() <= candidate.clear_window)
        .map(|candidate| (candidate.confidence, candidate.source))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ObservationPhase {
    Prepared,
    Injected,
    Observing,
    SendCandidate,
    WaitingForClear,
    Completed,
    Abandoned,
}

#[derive(Clone)]
struct PendingFinalDraft {
    epoch: u64,
    final_text: String,
    baseline: String,
    correction_after: Option<String>,
    confidence: &'static str,
    source: &'static str,
}

struct ActiveSession {
    epoch: u64,
    history_id: String,
    target: ActivationTarget,
    expected_after_injection: String,
    original_prefix: String,
    original_suffix: String,
    observer: PlatformObserver,
    armed: bool,
    phase: ObservationPhase,
    revision: u64,
    last_nonempty: String,
    candidate: Option<SendCandidate>,
}

#[derive(Default)]
pub(crate) struct FinalDraftRuntime {
    epoch: AtomicU64,
    active: Mutex<Option<ActiveSession>>,
    pending: Mutex<HashMap<String, PendingFinalDraft>>,
}

fn observation_allowed(app: &AppHandle, identity: &AppIdentity) -> bool {
    let state = app.state::<crate::state::RuntimeState>();
    let Ok(settings) = state.app_settings.lock() else {
        return false;
    };
    let prefs = &settings.history_prefs;
    prefs.get("enabled").and_then(serde_json::Value::as_bool) != Some(false)
        && crate::application::learning::observation_enabled(&state)
        && !prefs
            .get("excludedApps")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(serde_json::Value::as_str)
            .any(|value| {
                value.eq_ignore_ascii_case(identity.process_name.trim())
                    || value.eq_ignore_ascii_case(identity.app_name.trim())
            })
}

fn replacement_parts(snapshot: &DraftSnapshot, injected: &str) -> Option<(String, String, String)> {
    if snapshot.truncated {
        return None;
    }
    let source: Vec<u16> = snapshot.value.encode_utf16().collect();
    let end = snapshot
        .selection_location
        .checked_add(snapshot.selection_length)?;
    if end > source.len() {
        return None;
    }
    let prefix = String::from_utf16(&source[..snapshot.selection_location]).ok()?;
    let suffix = String::from_utf16(&source[end..]).ok()?;
    let expected = format!("{prefix}{injected}{suffix}");
    (expected.chars().count() <= MAX_DRAFT_CHARS).then_some((expected, prefix, suffix))
}

pub(crate) async fn prepare(
    app: &AppHandle,
    history_id: Option<&str>,
    target: ActivationTarget,
    identity: &AppIdentity,
    injected: &str,
) -> Option<u64> {
    let history_id = history_id.filter(|value| !value.is_empty())?;
    if injected.is_empty() || !observation_allowed(app, identity) {
        return None;
    }
    let runtime = &app
        .state::<crate::state::RuntimeState>()
        .final_draft_runtime;
    let epoch = runtime.epoch.fetch_add(1, Ordering::AcqRel) + 1;
    if let Ok(mut active) = runtime.active.lock() {
        active.take();
    }
    if let Ok(mut pending) = runtime.pending.lock() {
        pending.retain(|_, value| value.epoch >= epoch);
    }
    let process_id = target.process_id;
    let started = tauri::async_runtime::spawn_blocking(move || {
        PlatformDraftObserverPort::start(process_id, MAX_DRAFT_CHARS)
    })
    .await;
    let (observer, snapshot, events) = match started {
        Ok(Ok(result)) => result,
        Ok(Err(_error)) => {
            crate::application::diagnostics::event(
                "debug",
                "finalDraft.prepareSkipped",
                json!({"platform":std::env::consts::OS,"processId":process_id,"errorCode":"observerUnavailable"}),
            );
            return None;
        }
        Err(_error) => {
            crate::application::diagnostics::event(
                "warn",
                "finalDraft.prepareFailed",
                json!({"platform":std::env::consts::OS,"processId":process_id,"errorCode":"observerStartTaskFailed"}),
            );
            return None;
        }
    };
    let Some((expected_after_injection, original_prefix, original_suffix)) =
        replacement_parts(&snapshot, injected)
    else {
        crate::application::diagnostics::event(
            "debug",
            "finalDraft.prepareSkipped",
            json!({"historyId":history_id,"errorCode":"unattributableInitialValue"}),
        );
        return None;
    };
    let session = ActiveSession {
        epoch,
        history_id: history_id.to_owned(),
        target,
        expected_after_injection,
        original_prefix,
        original_suffix,
        observer,
        armed: false,
        phase: ObservationPhase::Prepared,
        revision: 0,
        last_nonempty: snapshot.value,
        candidate: None,
    };
    if let Ok(mut active) = runtime.active.lock() {
        if runtime.epoch.load(Ordering::Acquire) != epoch {
            return None;
        }
        *active = Some(session);
    } else {
        return None;
    }
    crate::application::diagnostics::event(
        "debug",
        "finalDraft.prepared",
        json!({"historyId":history_id,"sessionId":epoch,"platform":std::env::consts::OS}),
    );
    spawn_event_loop(app.clone(), epoch, events);
    Some(epoch)
}

pub(crate) fn mark_injected(app: &AppHandle, epoch: u64) {
    let runtime = &app
        .state::<crate::state::RuntimeState>()
        .final_draft_runtime;
    let mut cancel_reason = None;
    if let Ok(mut active) = runtime.active.lock() {
        let Some(session) = active.as_mut().filter(|session| session.epoch == epoch) else {
            return;
        };
        if crate::active_app_context::activation_target().is_some_and(|current| {
            !crate::active_app_context::same_activation_target(current, session.target)
        }) {
            cancel_reason = Some("targetChanged");
        } else {
            match session.observer.snapshot() {
                Ok(snapshot)
                    if !snapshot.truncated
                        && snapshot.value == session.expected_after_injection =>
                {
                    session.armed = true;
                    session.phase = ObservationPhase::Injected;
                    session.last_nonempty = snapshot.value;
                }
                Ok(_) => cancel_reason = Some("injectionMismatch"),
                Err(_) => cancel_reason = Some("snapshotUnavailable"),
            }
        }
        if cancel_reason.is_some() {
            active.take();
        }
    }
    if let Some(reason) = cancel_reason {
        crate::application::diagnostics::event(
            "debug",
            "finalDraft.abandoned",
            json!({"sessionId":epoch,"errorCode":reason}),
        );
        return;
    }
    crate::application::diagnostics::event(
        "debug",
        "finalDraft.observing",
        json!({"sessionId":epoch}),
    );
    if let Ok(mut active) = runtime.active.lock() {
        if let Some(session) = active.as_mut().filter(|session| session.epoch == epoch) {
            session.phase = ObservationPhase::Observing;
        }
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(OBSERVATION_TIMEOUT).await;
        abandon(&app, epoch, "timeout");
    });
}

pub(crate) fn cancel(app: &AppHandle, epoch: u64, reason: &'static str) {
    abandon(app, epoch, reason);
}

pub(crate) fn cancel_current(app: &AppHandle, reason: &'static str) {
    let state = app.state::<crate::state::RuntimeState>();
    let runtime = &state.final_draft_runtime;
    runtime.epoch.fetch_add(1, Ordering::AcqRel);
    let removed = runtime
        .active
        .lock()
        .ok()
        .and_then(|mut active| active.take())
        .is_some();
    if removed {
        crate::application::diagnostics::event(
            "debug",
            "finalDraft.abandoned",
            json!({"errorCode":reason}),
        );
    }
}

pub(crate) fn cancel_history(app: &AppHandle, history_id: &str) {
    let state = app.state::<crate::state::RuntimeState>();
    let runtime = &state.final_draft_runtime;
    let removed = runtime
        .active
        .lock()
        .ok()
        .and_then(|mut active| {
            if active
                .as_ref()
                .is_some_and(|session| session.history_id == history_id)
            {
                active.take()
            } else {
                None
            }
        })
        .is_some();
    if removed {
        runtime.epoch.fetch_add(1, Ordering::AcqRel);
        crate::application::diagnostics::event(
            "debug",
            "finalDraft.abandoned",
            json!({"historyId":history_id,"errorCode":"historyDeleted"}),
        );
    }
    if let Ok(mut pending) = runtime.pending.lock() {
        pending.remove(history_id);
    };
}

pub(crate) fn mark_auto_enter(app: &AppHandle) {
    let state = app.state::<crate::state::RuntimeState>();
    let runtime = &state.final_draft_runtime;
    if let Ok(mut active) = runtime.active.lock() {
        if let Some(session) = active.as_mut().filter(|session| session.armed) {
            session.candidate = Some(SendCandidate {
                at: Instant::now(),
                clear_window: KEYBOARD_CLEAR_WINDOW,
                confidence: "high",
                source: "autoEnter",
                value: session.last_nonempty.clone(),
                revision: session.revision,
            });
            session.phase = ObservationPhase::SendCandidate;
        }
    };
}

fn spawn_event_loop(
    app: AppHandle,
    epoch: u64,
    mut events: mpsc::UnboundedReceiver<ObserverEvent>,
) {
    tauri::async_runtime::spawn(async move {
        while let Some(event) = events.recv().await {
            if handle_event(&app, epoch, event) {
                return;
            }
        }
        abandon(&app, epoch, "observerClosed");
    });
}

fn handle_event(app: &AppHandle, epoch: u64, event: ObserverEvent) -> bool {
    let runtime = &app
        .state::<crate::state::RuntimeState>()
        .final_draft_runtime;
    let mut completed = None;
    let mut should_end = false;
    if let Ok(mut active) = runtime.active.lock() {
        let Some(session) = active.as_mut().filter(|session| session.epoch == epoch) else {
            return true;
        };
        match event.kind {
            ObserverEventKind::FocusChanged => {
                if let Some(candidate) = session
                    .candidate
                    .as_ref()
                    .filter(|candidate| candidate.at.elapsed() <= candidate.clear_window)
                {
                    completed = Some((
                        session.history_id.clone(),
                        candidate.value.clone(),
                        session.expected_after_injection.clone(),
                        session.original_prefix.clone(),
                        session.original_suffix.clone(),
                        "medium",
                        candidate.source,
                        candidate.at.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
                    ));
                    session.phase = ObservationPhase::Completed;
                } else {
                    session.phase = ObservationPhase::Abandoned;
                }
                should_end = true;
            }
            ObserverEventKind::ReturnPressed if session.armed => {
                if let Some(value) = event.value.filter(|value| !value.is_empty()) {
                    session.revision = session.revision.saturating_add(1);
                    session.last_nonempty = value;
                }
                if !session.candidate.as_ref().is_some_and(|candidate| {
                    candidate.source == "autoEnter"
                        && candidate.at.elapsed() <= candidate.clear_window
                }) {
                    session.candidate = Some(SendCandidate {
                        at: Instant::now(),
                        clear_window: KEYBOARD_CLEAR_WINDOW,
                        confidence: "high",
                        source: "keyboard",
                        value: session.last_nonempty.clone(),
                        revision: session.revision,
                    });
                }
                session.phase = ObservationPhase::SendCandidate;
            }
            ObserverEventKind::Clicked if session.armed => {
                if let Some(value) = event.value.filter(|value| !value.is_empty()) {
                    session.revision = session.revision.saturating_add(1);
                    session.last_nonempty = value;
                }
                session.candidate = Some(SendCandidate {
                    at: Instant::now(),
                    clear_window: CLICK_CLEAR_WINDOW,
                    confidence: "medium",
                    source: "click",
                    value: session.last_nonempty.clone(),
                    revision: session.revision,
                });
                session.phase = ObservationPhase::SendCandidate;
            }
            ObserverEventKind::ValueChanged if session.armed => match event.value {
                Some(value) if value.is_empty() => {
                    if let Some(candidate) = session
                        .candidate
                        .as_ref()
                        .filter(|candidate| candidate.at.elapsed() <= candidate.clear_window)
                    {
                        completed = Some((
                            session.history_id.clone(),
                            candidate.value.clone(),
                            session.expected_after_injection.clone(),
                            session.original_prefix.clone(),
                            session.original_suffix.clone(),
                            candidate.confidence,
                            candidate.source,
                            candidate.at.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
                        ));
                        session.phase = ObservationPhase::Completed;
                    } else {
                        session.phase = ObservationPhase::Abandoned;
                    }
                    should_end = true;
                }
                Some(value) => {
                    session.revision = session.revision.saturating_add(1);
                    session.last_nonempty = value;
                    if let Some(candidate) = session.candidate.as_mut() {
                        if candidate.at.elapsed() <= candidate.clear_window {
                            candidate.value = session.last_nonempty.clone();
                            candidate.revision = session.revision;
                            session.phase = ObservationPhase::WaitingForClear;
                        }
                    }
                }
                None => {
                    if let Some(candidate) = session
                        .candidate
                        .as_ref()
                        .filter(|candidate| candidate.at.elapsed() <= candidate.clear_window)
                    {
                        completed = Some((
                            session.history_id.clone(),
                            candidate.value.clone(),
                            session.expected_after_injection.clone(),
                            session.original_prefix.clone(),
                            session.original_suffix.clone(),
                            "medium",
                            candidate.source,
                            candidate.at.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
                        ));
                        session.phase = ObservationPhase::Completed;
                    } else {
                        session.phase = ObservationPhase::Abandoned;
                    }
                    should_end = true;
                }
            },
            _ => {}
        }
        if should_end {
            active.take();
        }
    }
    if let Some((
        history_id,
        final_text,
        baseline,
        prefix,
        suffix,
        confidence,
        source,
        candidate_to_clear_ms,
    )) = completed
    {
        let correction_after = attributable_middle(&final_text, &prefix, &suffix);
        crate::application::diagnostics::event(
            "info",
            "finalDraft.completed",
            json!({
                "historyId":history_id,
                "sessionId":epoch,
                "confidence":confidence,
                "source":source,
                "candidateToClearMs":candidate_to_clear_ms,
                "finalTextChars":final_text.chars().count(),
                "finalTextFingerprint":crate::application::diagnostics::fingerprint(&final_text),
            }),
        );
        crate::application::diagnostics::content_event(
            "finalDraft.completed",
            json!({"historyId":history_id,"finalText":final_text}),
        );
        save_or_queue(
            app,
            epoch,
            history_id,
            final_text,
            baseline,
            correction_after,
            confidence,
            source,
        );
    } else if should_end {
        crate::application::diagnostics::event(
            "debug",
            "finalDraft.abandoned",
            json!({"sessionId":epoch,"errorCode":"noSendConfirmation"}),
        );
    }
    should_end
}

fn save_or_queue(
    app: &AppHandle,
    epoch: u64,
    history_id: String,
    final_text: String,
    baseline: String,
    correction_after: Option<String>,
    confidence: &'static str,
    source: &'static str,
) {
    match crate::application::history::record_observed_final_text(
        app,
        &history_id,
        &final_text,
        &baseline,
        correction_after.as_deref(),
        confidence,
        source,
    ) {
        Ok(true) => {}
        Ok(false) => {
            let runtime = &app
                .state::<crate::state::RuntimeState>()
                .final_draft_runtime;
            if let Ok(mut pending) = runtime.pending.lock() {
                pending.insert(
                    history_id.clone(),
                    PendingFinalDraft {
                        epoch,
                        final_text,
                        baseline,
                        correction_after,
                        confidence,
                        source,
                    },
                );
            }
            crate::application::diagnostics::event(
                "debug",
                "finalDraft.pending",
                json!({"historyId":history_id,"sessionId":epoch}),
            );
        }
        Err(_) => crate::application::diagnostics::event(
            "warn",
            "finalDraft.persistFailed",
            json!({"historyId":history_id,"sessionId":epoch,"errorCode":"historyWriteFailed"}),
        ),
    }
}

pub(crate) fn consume_pending(app: &AppHandle, history_id: &str) {
    let runtime = &app
        .state::<crate::state::RuntimeState>()
        .final_draft_runtime;
    let pending = runtime
        .pending
        .lock()
        .ok()
        .and_then(|mut pending| pending.remove(history_id));
    let Some(pending) = pending else {
        return;
    };
    match crate::application::history::record_observed_final_text(
        app,
        history_id,
        &pending.final_text,
        &pending.baseline,
        pending.correction_after.as_deref(),
        pending.confidence,
        pending.source,
    ) {
        Ok(true) => crate::application::diagnostics::event(
            "debug",
            "finalDraft.pendingConsumed",
            json!({"historyId":history_id,"sessionId":pending.epoch}),
        ),
        Ok(false) => {
            if let Ok(mut values) = runtime.pending.lock() {
                values.insert(history_id.to_owned(), pending);
            }
        }
        Err(_) => crate::application::diagnostics::event(
            "warn",
            "finalDraft.pendingPersistFailed",
            json!({"historyId":history_id,"sessionId":pending.epoch,"errorCode":"historyWriteFailed"}),
        ),
    }
}

fn attributable_middle(final_text: &str, prefix: &str, suffix: &str) -> Option<String> {
    let rest = final_text.strip_prefix(prefix)?;
    let middle = rest.strip_suffix(suffix)?;
    Some(middle.to_owned())
}

fn abandon(app: &AppHandle, epoch: u64, reason: &'static str) {
    let runtime = &app
        .state::<crate::state::RuntimeState>()
        .final_draft_runtime;
    let removed = runtime
        .active
        .lock()
        .ok()
        .and_then(|mut active| {
            active
                .as_ref()
                .is_some_and(|session| session.epoch == epoch)
                .then(|| active.take())
                .flatten()
        })
        .is_some();
    if removed {
        crate::application::diagnostics::event(
            "debug",
            "finalDraft.abandoned",
            json!({"sessionId":epoch,"errorCode":reason}),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attributes_injection_with_existing_text_and_selection() {
        let snapshot = DraftSnapshot {
            value: "你好旧内容结尾".into(),
            selection_location: 2,
            selection_length: 3,
            truncated: false,
        };
        let (expected, prefix, suffix) = replacement_parts(&snapshot, "新文本").unwrap();
        assert_eq!(expected, "你好新文本结尾");
        assert_eq!(
            attributable_middle("你好新文本！结尾", &prefix, &suffix).as_deref(),
            Some("新文本！")
        );
    }

    #[test]
    fn rejects_invalid_utf16_selection_and_truncated_values() {
        let invalid = DraftSnapshot {
            value: "🙂".into(),
            selection_location: 1,
            selection_length: 0,
            truncated: false,
        };
        assert!(replacement_parts(&invalid, "x").is_none());
        assert!(replacement_parts(
            &DraftSnapshot {
                truncated: true,
                ..invalid
            },
            "x"
        )
        .is_none());
    }

    #[test]
    fn rejects_drafts_over_the_observation_limit() {
        let snapshot = DraftSnapshot {
            value: String::new(),
            selection_location: 0,
            selection_length: 0,
            truncated: false,
        };
        assert!(replacement_parts(&snapshot, &"字".repeat(MAX_DRAFT_CHARS + 1)).is_none());
    }

    #[test]
    fn does_not_attribute_when_original_surroundings_changed() {
        assert_eq!(attributable_middle("不同前缀结果尾", "前缀", "尾"), None);
    }

    #[test]
    fn return_and_click_candidates_have_fixed_confidence() {
        let keyboard = SendCandidate {
            at: Instant::now(),
            clear_window: KEYBOARD_CLEAR_WINDOW,
            confidence: "high",
            source: "keyboard",
            value: "草稿".into(),
            revision: 1,
        };
        let click = SendCandidate {
            at: Instant::now(),
            clear_window: CLICK_CLEAR_WINDOW,
            confidence: "medium",
            source: "click",
            value: "草稿".into(),
            revision: 1,
        };
        assert_eq!(
            confirmed_candidate(Some(&keyboard)),
            Some(("high", "keyboard"))
        );
        assert_eq!(confirmed_candidate(Some(&click)), Some(("medium", "click")));
        assert_eq!(confirmed_candidate(None), None);
    }

    #[test]
    fn expired_send_candidate_does_not_confirm_a_later_clear() {
        let expired = SendCandidate {
            at: Instant::now() - KEYBOARD_CLEAR_WINDOW - Duration::from_millis(1),
            clear_window: KEYBOARD_CLEAR_WINDOW,
            confidence: "high",
            source: "keyboard",
            value: "草稿".into(),
            revision: 1,
        };
        assert_eq!(confirmed_candidate(Some(&expired)), None);
    }

    #[test]
    fn automatic_enter_is_high_confidence_and_plain_clear_is_not_a_send() {
        let automatic = SendCandidate {
            at: Instant::now(),
            clear_window: KEYBOARD_CLEAR_WINDOW,
            confidence: "high",
            source: "autoEnter",
            value: "草稿".into(),
            revision: 1,
        };
        assert_eq!(
            confirmed_candidate(Some(&automatic)),
            Some(("high", "autoEnter"))
        );
        assert_eq!(confirmed_candidate(None), None);
    }
}
