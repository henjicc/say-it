use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::HostRuntimeRecorder;

#[derive(Clone, Default)]
pub(super) struct AuditRecorder(pub(super) Arc<Mutex<Vec<Value>>>);

impl HostRuntimeRecorder for AuditRecorder {
    fn record(&self, event: Value) {
        self.0.lock().expect("audit recorder lock").push(event);
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct LiveCaseResult {
    pub(super) case_id: String,
    pub(super) model: String,
    pub(super) protocol: String,
    pub(super) success: bool,
    pub(super) latency_ms: u128,
    pub(super) content_hash: Option<String>,
    pub(super) task_id_hash: Option<String>,
    pub(super) event_types: Vec<String>,
    pub(super) usage: Option<Value>,
    pub(super) finish_reason: Option<String>,
    pub(super) resource_counts: (usize, usize, usize),
    pub(super) error_class: Option<&'static str>,
    pub(super) error_hash: Option<String>,
}

pub(super) fn fingerprint(value: &str) -> String {
    let digest = Sha256::digest(value.as_bytes());
    format!("{:x}", digest)[..16].to_string()
}

fn event_value(event: &Value) -> &Value {
    event.get("event").unwrap_or(event)
}

fn event_types(events: &[Value]) -> Vec<String> {
    let mut types = events
        .iter()
        .filter_map(|event| event_value(event).get("type").and_then(Value::as_str))
        .map(str::to_string)
        .collect::<Vec<_>>();
    types.sort();
    types.dedup();
    types
}

fn provider_task_hash(events: &[Value]) -> Option<String> {
    events.iter().find_map(|event| {
        let event = event_value(event);
        event
            .get("taskId")
            .or_else(|| event.get("sessionId"))
            .and_then(Value::as_str)
            .map(fingerprint)
    })
}

fn numeric_usage(value: Option<&Value>) -> Option<Value> {
    let object = value?.as_object()?;
    let values = object
        .iter()
        .filter(|(_, value)| value.is_number() || value.is_null())
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect::<serde_json::Map<_, _>>();
    (!values.is_empty()).then_some(Value::Object(values))
}

pub(super) fn error_class(error: &str) -> &'static str {
    let error = error.to_ascii_lowercase();
    if error.contains("timeout") || error.contains("超时") {
        "timeout"
    } else if error.contains("not configured")
        || error.contains("api_key_missing")
        || error.contains("未配置")
        || error.contains("credential_missing")
    {
        "missing_credential"
    } else if error.contains("401") || error.contains("unauthorized") {
        "unauthorized"
    } else if error.contains("403") || error.contains("forbidden") {
        "forbidden"
    } else if error.contains("429") || error.contains("rate") && error.contains("limit") {
        "rate_limited"
    } else if error.contains("cancel") || error.contains("取消") || error.contains("interrupted")
    {
        "cancelled"
    } else {
        "provider_or_runtime_error"
    }
}

pub(super) fn outcome(
    case_id: &str,
    model: &str,
    protocol: &str,
    started: Instant,
    result: Result<Value, String>,
    events: Vec<Value>,
    counts: (usize, usize, usize),
) -> LiveCaseResult {
    match result {
        Ok(value) => {
            let content = value
                .get("text")
                .or_else(|| value.get("output"))
                .and_then(Value::as_str)
                .or_else(|| {
                    value
                        .get("translations")
                        .and_then(Value::as_array)
                        .and_then(|items| items.first())
                        .and_then(|item| item.get("text"))
                        .and_then(Value::as_str)
                })
                .unwrap_or_default();
            LiveCaseResult {
                case_id: case_id.into(),
                model: model.into(),
                protocol: protocol.into(),
                success: !content.trim().is_empty(),
                latency_ms: started.elapsed().as_millis(),
                content_hash: (!content.is_empty()).then(|| fingerprint(content)),
                task_id_hash: provider_task_hash(&events),
                event_types: event_types(&events),
                usage: numeric_usage(value.get("usage")),
                finish_reason: value
                    .get("finishReason")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                resource_counts: counts,
                error_class: None,
                error_hash: None,
            }
        }
        Err(error) => LiveCaseResult {
            case_id: case_id.into(),
            model: model.into(),
            protocol: protocol.into(),
            success: false,
            latency_ms: started.elapsed().as_millis(),
            content_hash: None,
            task_id_hash: provider_task_hash(&events),
            event_types: event_types(&events),
            usage: None,
            finish_reason: None,
            resource_counts: counts,
            error_class: Some(error_class(&error)),
            error_hash: Some(fingerprint(&error)),
        },
    }
}
