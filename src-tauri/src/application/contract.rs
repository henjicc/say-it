use crate::state::RuntimeState;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::atomic::{AtomicU64, Ordering};
use tauri::State;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ApplicationError {
    pub(crate) code: String,
    pub(crate) message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) details: Option<Value>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum DomainRunState {
    FrontendOwned,
    Idle,
    Running,
    Stopping,
    Failed,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DomainSnapshot {
    pub(crate) state: DomainRunState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) session_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConfigurationSummary {
    pub(crate) default_provider_id: String,
    pub(crate) dictation_shortcut: String,
    pub(crate) subtitle_shortcut: String,
    pub(crate) startup_silent: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AppSnapshot {
    pub(crate) revision: u64,
    pub(crate) configuration: ConfigurationSummary,
    pub(crate) settings: crate::application::settings::AppSettings,
    pub(crate) dictation: DomainSnapshot,
    pub(crate) subtitles: DomainSnapshot,
    pub(crate) transcription: DomainSnapshot,
    pub(crate) comparison: DomainSnapshot,
    pub(crate) audio_lab: DomainSnapshot,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DomainEventEnvelope {
    pub(crate) revision: u64,
    pub(crate) domain: String,
    pub(crate) event_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) session_id: Option<String>,
    pub(crate) payload: Value,
}

/// 后续应用服务使用的最小外部能力边界。当前阶段只固化契约，不改变现有运行路径。
#[allow(dead_code)]
pub(crate) trait ProviderPort {}
#[allow(dead_code)]
pub(crate) trait AudioInputPort {}
#[allow(dead_code)]
pub(crate) trait TextInjectionPort {}
#[allow(dead_code)]
pub(crate) trait IndicatorPort {}
#[allow(dead_code)]
pub(crate) trait PersistencePort {}
#[allow(dead_code)]
pub(crate) trait EventPublisherPort {
    fn publish(&self, event: DomainEventEnvelope) -> Result<(), ApplicationError>;
}

fn frontend_owned() -> DomainSnapshot {
    DomainSnapshot {
        state: DomainRunState::FrontendOwned,
        session_id: None,
    }
}

#[tauri::command]
pub(crate) fn get_app_snapshot(state: State<'_, RuntimeState>) -> Result<AppSnapshot, String> {
    let providers = state
        .providers
        .lock()
        .map_err(|_| "provider settings lock failed")?;
    let dictation = state
        .dictation
        .lock()
        .map_err(|_| "dictation settings lock failed")?;
    let subtitle = state
        .subtitle_shortcut
        .lock()
        .map_err(|_| "subtitle shortcut lock failed")?;
    let startup = state
        .startup
        .lock()
        .map_err(|_| "startup settings lock failed")?;
    let active_transcriptions = state
        .transcriptions
        .lock()
        .map_err(|_| "transcription state lock failed")?
        .len();
    let revision = state.snapshot_revision.load(Ordering::Acquire);
    let settings = state.app_settings.lock().map_err(|_| "app settings lock failed")?.clone();

    Ok(AppSnapshot {
        revision,
        configuration: ConfigurationSummary {
            default_provider_id: providers.defaults.asr.clone(),
            dictation_shortcut: dictation.key_code.clone(),
            subtitle_shortcut: subtitle.key_code.clone(),
            startup_silent: startup.silent_start,
        },
        settings,
        dictation: crate::application::dictation::domain_snapshot(&state)?,
        subtitles: crate::application::subtitles::domain_snapshot(&state)?,
        transcription: DomainSnapshot {
            state: if active_transcriptions == 0 {
                DomainRunState::Idle
            } else {
                DomainRunState::Running
            },
            session_id: None,
        },
        comparison: frontend_owned(),
        audio_lab: frontend_owned(),
    })
}

#[allow(dead_code)]
pub(crate) fn next_revision(revision: &AtomicU64) -> u64 {
    revision.fetch_add(1, Ordering::AcqRel) + 1
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn event_envelope_uses_camel_case_and_keeps_revision_identity() {
        let event = DomainEventEnvelope {
            revision: 7,
            domain: "dictation".into(),
            event_type: "stateChanged".into(),
            session_id: Some("session-1".into()),
            payload: json!({"state": "running"}),
        };
        let value = serde_json::to_value(event).unwrap();
        assert_eq!(value["eventType"], "stateChanged");
        assert_eq!(value["sessionId"], "session-1");
        assert_eq!(value["revision"], 7);
        assert!(value.get("event_type").is_none());
    }

    #[test]
    fn snapshot_serializes_frontend_owned_state_and_monotonic_revision() {
        let revision = AtomicU64::new(0);
        assert_eq!(next_revision(&revision), 1);
        assert_eq!(next_revision(&revision), 2);
        assert_eq!(
            serde_json::to_value(frontend_owned()).unwrap(),
            json!({"state":"frontendOwned"})
        );
    }
}
