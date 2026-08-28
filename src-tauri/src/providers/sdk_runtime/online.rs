use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use serde_json::{json, Value};
use tokio::sync::mpsc;

use super::{HostRuntimeRecorder, SdkHostBindings};
use crate::providers::credential_store::{CredentialKey, CredentialStoreHandle};
use crate::providers::plugin::PluginRuntimeSpec;
use crate::providers::plugin_runtime::JsProviderRuntime;
use crate::providers::ProviderProfile;

const BUILTIN_SOURCE: &str = include_str!("../../../../sdk-runtime/builtin-online-entry.js");
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(45);
const FILE_ASR_TIMEOUT: Duration = Duration::from_secs(30 * 60);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuiltinSdkScope {
    SpeechRecognition,
    Translation,
    Groq,
}

impl BuiltinSdkScope {
    fn provider_id(self) -> &'static str {
        match self {
            Self::SpeechRecognition | Self::Translation => "bailian",
            Self::Groq => "groq",
        }
    }

    fn credential_owner(self) -> &'static str {
        match self {
            Self::SpeechRecognition | Self::Translation => "bailian",
            Self::Groq => "llm-groq",
        }
    }

    fn credential_scope(self) -> &'static str {
        match self {
            Self::SpeechRecognition => "speech-recognition",
            Self::Translation => "translation",
            Self::Groq => "llm",
        }
    }

    fn allowed_hosts(self) -> Vec<String> {
        match self {
            Self::SpeechRecognition | Self::Translation => vec!["*.aliyuncs.com".into()],
            Self::Groq => vec!["api.groq.com".into()],
        }
    }
}

#[derive(Clone)]
struct StructuredRecorder;

impl HostRuntimeRecorder for StructuredRecorder {
    fn record(&self, event: Value) {
        let kind = event
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("sdk.runtime");
        crate::development_debug_log("sdk-runtime", format_args!("event={kind} metadata={event}"));
    }
}

pub struct BuiltinSdkRuntime {
    runtime: JsProviderRuntime,
    timeout: Duration,
}

impl BuiltinSdkRuntime {
    pub fn create(
        profile: &ProviderProfile,
        credentials: CredentialStoreHandle,
        scope: BuiltinSdkScope,
        request_id: impl Into<String>,
        cancelled: Arc<AtomicBool>,
        inputs: HashMap<String, PathBuf>,
    ) -> Result<Self, String> {
        Self::create_with_event_sender(
            profile,
            credentials,
            scope,
            request_id,
            cancelled,
            inputs,
            None,
        )
    }

    pub fn create_with_event_sender(
        profile: &ProviderProfile,
        credentials: CredentialStoreHandle,
        scope: BuiltinSdkScope,
        request_id: impl Into<String>,
        cancelled: Arc<AtomicBool>,
        inputs: HashMap<String, PathBuf>,
        event_tx: Option<mpsc::Sender<Value>>,
    ) -> Result<Self, String> {
        Self::create_with_recorder(
            profile,
            credentials,
            scope,
            request_id,
            cancelled,
            inputs,
            event_tx,
            Arc::new(StructuredRecorder),
        )
    }

    fn create_with_recorder(
        profile: &ProviderProfile,
        credentials: CredentialStoreHandle,
        scope: BuiltinSdkScope,
        request_id: impl Into<String>,
        cancelled: Arc<AtomicBool>,
        inputs: HashMap<String, PathBuf>,
        event_tx: Option<mpsc::Sender<Value>>,
        recorder: Arc<dyn HostRuntimeRecorder>,
    ) -> Result<Self, String> {
        let request_id = request_id.into();
        let spec = runtime_spec(scope)?;
        let credential_key = CredentialKey::provider(scope.credential_owner(), "apiKey")?;
        let bindings = SdkHostBindings {
            owner_id: format!("builtin:{}", scope.provider_id()),
            provider_id: scope.provider_id().into(),
            request_id,
            credential_scopes: HashSet::from([scope.credential_scope().into()]),
            credential_key,
            credentials,
            recorder,
        };
        let runtime = JsProviderRuntime::create_with_sdk_bindings_and_event_sender(
            spec,
            profile,
            DEFAULT_TIMEOUT,
            cancelled,
            inputs,
            event_tx,
            bindings,
        )?;
        Ok(Self {
            runtime,
            timeout: DEFAULT_TIMEOUT,
        })
    }

    #[cfg(test)]
    pub(super) fn create_for_live_test(
        profile: &ProviderProfile,
        credentials: CredentialStoreHandle,
        scope: BuiltinSdkScope,
        request_id: impl Into<String>,
        cancelled: Arc<AtomicBool>,
        inputs: HashMap<String, PathBuf>,
        recorder: Arc<dyn HostRuntimeRecorder>,
    ) -> Result<Self, String> {
        Self::create_with_recorder(
            profile,
            credentials,
            scope,
            request_id,
            cancelled,
            inputs,
            None,
            recorder,
        )
    }

    pub fn execute_capability(
        &self,
        source: &str,
        module_id: &str,
        input: Value,
        request_id: &str,
    ) -> Result<Value, String> {
        self.execute_capability_with_timeout(source, module_id, input, request_id, self.timeout)
    }

    fn execute_capability_with_timeout(
        &self,
        source: &str,
        module_id: &str,
        input: Value,
        request_id: &str,
        timeout: Duration,
    ) -> Result<Value, String> {
        self.runtime.call(
            "invoke",
            &json!({
                "operation": "capability.execute",
                "source": source,
                "moduleId": module_id,
                "input": input,
                "requestId": request_id,
                "timeoutMs": timeout.as_millis(),
            }),
            timeout,
        )
    }

    pub fn run_groq(
        &self,
        input: Value,
        request_id: &str,
        emit_events: bool,
    ) -> Result<Value, String> {
        self.runtime.call(
            "invoke",
            &json!({
                "operation": "groq.run",
                "input": input,
                "requestId": request_id,
                "timeoutMs": self.timeout.as_millis(),
                "emitEvents": emit_events,
            }),
            self.timeout,
        )
    }

    pub fn discover_groq(&self) -> Result<Value, String> {
        self.runtime.call(
            "invoke",
            &json!({
                "operation": "groq.discover",
                "timeoutMs": self.timeout.as_millis(),
            }),
            self.timeout,
        )
    }

    pub fn realtime_start(
        &self,
        module_id: &str,
        input: Value,
        request_id: &str,
    ) -> Result<(), String> {
        self.runtime
            .call(
                "realtimeStart",
                &json!({
                    "moduleId": module_id,
                    "input": input,
                    "requestId": request_id,
                    "timeoutMs": self.timeout.as_millis(),
                }),
                self.timeout,
            )
            .map(|_| ())
    }

    pub fn realtime_audio(&self, bytes: Vec<u8>) -> Result<(), String> {
        self.runtime.call_audio(bytes)
    }

    pub fn dispatch_host_events(&self) -> Result<(), String> {
        self.runtime.dispatch_host_events()
    }

    pub fn take_events(&self) -> Vec<Value> {
        self.runtime.take_events()
    }

    pub fn realtime_finish(&self) -> Result<Value, String> {
        self.runtime
            .call("realtimeFinish", &Value::Null, self.timeout)
    }

    pub fn realtime_stop(&self) -> Result<(), String> {
        self.runtime
            .call("realtimeStop", &Value::Null, Duration::from_secs(5))
            .map(|_| ())
    }

    #[cfg(test)]
    pub(super) fn resource_counts(&self) -> (usize, usize, usize) {
        self.runtime.sdk_resource_counts()
    }
}

pub async fn recognize_bailian_file(
    profile: ProviderProfile,
    credentials: CredentialStoreHandle,
    path: String,
    params: crate::providers::alibabacloud::TranscriptionParams,
    customization: crate::providers::RequestCustomization,
    cancelled: Option<Arc<AtomicBool>>,
) -> Result<crate::providers::alibabacloud::TranscriptionResult, String> {
    let request_id = format!("bailian-asr-{}", uuid::Uuid::new_v4());
    let model = params.model_id();
    let module_id = format!("bailian.speech-recognition.{model}");
    let vocabulary_id = profile
        .config
        .get("vocabularyIds")
        .and_then(|value| value.get(&model))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string);
    let language = (params.language_hints.len() == 1)
        .then(|| params.language_hints[0].trim().to_string())
        .filter(|value| !value.is_empty());
    let hints = params
        .language_hints
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    let input = json!({
        "audio": { "kind": "media-ref", "ref": "input-audio" },
        "language": language,
        "hints": hints,
        "timestamps": true,
        "options": {
            "context": customization.context.trim(),
            "vocabularyId": vocabulary_id,
            "diarizationEnabled": params.diarization_enabled,
            "speakerCount": params.speaker_count,
            "channelId": params.channel_id,
            "specialWordFilter": params.special_word_filter.trim(),
        },
    });
    let cancel = cancelled.unwrap_or_else(|| Arc::new(AtomicBool::new(false)));
    let value = tauri::async_runtime::spawn_blocking(move || {
        let runtime = BuiltinSdkRuntime::create(
            &profile,
            credentials,
            BuiltinSdkScope::SpeechRecognition,
            request_id.clone(),
            cancel,
            HashMap::from([("input-audio".into(), PathBuf::from(path))]),
        )?;
        runtime.execute_capability_with_timeout(
            "asr",
            &module_id,
            input,
            &request_id,
            FILE_ASR_TIMEOUT,
        )
    })
    .await
    .map_err(|error| format!("百炼 SDK 识别工作线程失败：{error}"))??;
    sdk_asr_to_legacy(value)
}

pub async fn translate_bailian<F>(
    profile: ProviderProfile,
    credentials: CredentialStoreHandle,
    model: String,
    text: String,
    source_language: String,
    target_language: String,
    mut on_delta: F,
) -> Result<String, String>
where
    F: FnMut(&str),
{
    let request_id = format!("bailian-translation-{}", uuid::Uuid::new_v4());
    let module_id = format!("bailian.translation.{model}");
    let input = json!({
        "source": text,
        "sourceLanguage": source_language,
        "targetLanguage": target_language,
        "options": { "stream": true },
    });
    let (event_tx, mut event_rx) = mpsc::channel(128);
    let task_request_id = request_id.clone();
    let mut task = tauri::async_runtime::spawn_blocking(move || {
        let runtime = BuiltinSdkRuntime::create_with_event_sender(
            &profile,
            credentials,
            BuiltinSdkScope::Translation,
            task_request_id.clone(),
            Arc::new(AtomicBool::new(false)),
            HashMap::new(),
            Some(event_tx),
        )?;
        runtime.execute_capability("translation", &module_id, input, &task_request_id)
    });
    let mut accumulated = String::new();
    loop {
        tokio::select! {
            event = event_rx.recv() => {
                if let Some(event) = event {
                    let event = event.get("event").unwrap_or(&event);
                    if event.get("type").and_then(Value::as_str) == Some("delta") {
                        if let Some(value) = event.get("accumulatedText").and_then(Value::as_str) {
                            accumulated.clear();
                            accumulated.push_str(value);
                        } else if let Some(value) = event.get("text").and_then(Value::as_str) {
                            accumulated.push_str(value);
                        }
                        on_delta(&accumulated);
                    }
                }
            }
            result = &mut task => {
                let value = result
                    .map_err(|error| format!("百炼翻译 SDK 工作线程失败：{error}"))??;
                let output = value
                    .get("translations")
                    .and_then(Value::as_array)
                    .and_then(|items| items.first())
                    .and_then(|item| item.get("text"))
                    .and_then(Value::as_str)
                    .ok_or("百炼翻译 SDK 未返回译文")?
                    .to_string();
                if accumulated != output {
                    on_delta(&output);
                }
                return Ok(output);
            }
        }
    }
}

fn sdk_asr_to_legacy(
    value: Value,
) -> Result<crate::providers::alibabacloud::TranscriptionResult, String> {
    let sentences = value
        .get("segments")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|segment| {
            let words = segment
                .get("words")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(|word| {
                    json!({
                        "beginTime": word.get("startMs").and_then(Value::as_u64).unwrap_or_default(),
                        "endTime": word.get("endMs").and_then(Value::as_u64).unwrap_or_default(),
                        "text": word.get("text").and_then(Value::as_str).unwrap_or_default(),
                    })
                })
                .collect::<Vec<_>>();
            json!({
                "beginTime": segment.get("startMs").and_then(Value::as_u64).unwrap_or_default(),
                "endTime": segment.get("endMs").and_then(Value::as_u64).unwrap_or_default(),
                "text": segment.get("text").and_then(Value::as_str).unwrap_or_default(),
                "words": words,
            })
        })
        .collect::<Vec<_>>();
    serde_json::from_value(json!({
        "durationMs": value.get("durationMs").cloned().unwrap_or(Value::Null),
        "transcripts": [{
            "channelId": Value::Null,
            "text": value.get("text").and_then(Value::as_str).unwrap_or_default(),
            "sentences": sentences,
        }],
    }))
    .map_err(|error| format!("百炼 SDK 识别结果映射失败：{error}"))
}

fn runtime_spec(scope: BuiltinSdkScope) -> Result<PluginRuntimeSpec, String> {
    let root = builtin_root()?;
    Ok(PluginRuntimeSpec {
        plugin_id: format!("builtin-sdk-{}", scope.provider_id()),
        source_namespace: "@henjicc/ai-sdk".into(),
        capabilities: vec![],
        secret_fields: vec!["apiKey".into()],
        credentials: None,
        root: root.clone(),
        entrypoint: root.join("connector/index.js"),
        permissions: vec!["network".into()],
        allowed_hosts: scope.allowed_hosts(),
        browser_session: None,
        data_dir: root.join("data").join(scope.provider_id()),
        trust: "builtin".into(),
    })
}

fn builtin_root() -> Result<PathBuf, String> {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    if let Some(root) = ROOT.get() {
        return Ok(root.clone());
    }
    let root = std::env::temp_dir().join("say-it-sdk-runtime-0.2.2");
    let connector = root.join("connector/index.js");
    std::fs::create_dir_all(connector.parent().unwrap_or(Path::new(".")))
        .map_err(|error| error.to_string())?;
    let current = std::fs::read_to_string(&connector).unwrap_or_default();
    if current != BUILTIN_SOURCE {
        std::fs::write(&connector, BUILTIN_SOURCE).map_err(|error| error.to_string())?;
    }
    let _ = ROOT.set(root.clone());
    Ok(root)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_sdk_asr_output_to_existing_transcription_contract() {
        let result = sdk_asr_to_legacy(json!({
            "text": "你好",
            "durationMs": 1200,
            "segments": [{
                "text": "你好",
                "startMs": 10,
                "endMs": 900,
                "words": [{ "text": "你", "startMs": 10, "endMs": 300 }],
            }],
        }))
        .unwrap();
        assert_eq!(result.duration_ms, Some(1200));
        assert_eq!(result.transcripts[0].text, "你好");
        assert_eq!(result.transcripts[0].sentences[0].begin_time, 10);
        assert_eq!(result.transcripts[0].sentences[0].words[0].text, "你");
    }
}
