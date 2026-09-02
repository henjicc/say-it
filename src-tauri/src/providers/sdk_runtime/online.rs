use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

use serde::Serialize;
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
pub struct BuiltinSdkScope {
    profile_id: &'static str,
    provider_id: &'static str,
    credential_owner: &'static str,
    credential_scope: &'static str,
    allowed_hosts: &'static [&'static str],
}

impl BuiltinSdkScope {
    pub const BAILIAN_SPEECH_RECOGNITION: Self = Self {
        profile_id: "bailian",
        provider_id: "bailian",
        credential_owner: "bailian",
        credential_scope: "speech-recognition",
        allowed_hosts: &["*.aliyuncs.com"],
    };
    pub const BAILIAN_TRANSLATION: Self = Self {
        profile_id: "bailian",
        provider_id: "bailian",
        credential_owner: "bailian",
        credential_scope: "translation",
        allowed_hosts: &["*.aliyuncs.com"],
    };
    pub const GROQ_LLM: Self = Self {
        profile_id: "llm-groq",
        provider_id: "groq",
        credential_owner: "llm-groq",
        credential_scope: "llm",
        allowed_hosts: &["api.groq.com"],
    };

    pub fn speech_recognition(profile: &ProviderProfile) -> Result<Self, String> {
        match profile.kind.as_str() {
            "sdk:bailian" => Ok(Self::BAILIAN_SPEECH_RECOGNITION),
            "sdk:volcengine" => Ok(Self {
                profile_id: "volcengine",
                provider_id: "volcengine",
                credential_owner: "volcengine",
                credential_scope: "speech-recognition",
                allowed_hosts: &["openspeech.bytedance.com"],
            }),
            "sdk:siliconflow" => Ok(Self {
                profile_id: "siliconflow",
                provider_id: "siliconflow",
                credential_owner: "siliconflow",
                credential_scope: "speech-recognition",
                allowed_hosts: &["api.siliconflow.cn"],
            }),
            "llm:groq" => Ok(Self {
                profile_id: "llm-groq",
                provider_id: "groq",
                credential_owner: "llm-groq",
                credential_scope: "speech-recognition",
                allowed_hosts: &["api.groq.com"],
            }),
            _ => Err(format!(
                "供应商 {} 没有内置 SDK 语音识别运行时",
                profile.display_name
            )),
        }
    }

    fn provider_id(self) -> &'static str {
        self.provider_id
    }

    fn credential_owner(self) -> &'static str {
        self.credential_owner
    }

    fn credential_scope(self) -> &'static str {
        self.credential_scope
    }

    fn allowed_hosts(self) -> Vec<String> {
        self.allowed_hosts
            .iter()
            .map(|host| (*host).into())
            .collect()
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SdkFileAsrOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    context: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    vocabulary_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    diarization_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    speaker_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    channel_id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    special_word_filter: Option<String>,
}

impl SdkFileAsrOptions {
    fn is_empty(&self) -> bool {
        self.context.is_none()
            && self.vocabulary_id.is_none()
            && self.diarization_enabled.is_none()
            && self.speaker_count.is_none()
            && self.channel_id.is_none()
            && self.special_word_filter.is_none()
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SdkFileAsrInput {
    audio: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    language: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    hints: Vec<String>,
    timestamps: bool,
    #[serde(skip_serializing_if = "SdkFileAsrOptions::is_empty")]
    options: SdkFileAsrOptions,
}

fn non_empty_string(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn sdk_file_asr_input(
    params: &crate::providers::alibabacloud::TranscriptionParams,
    customization: &crate::providers::RequestCustomization,
    vocabulary_id: Option<String>,
) -> Value {
    let hints = params
        .language_hints
        .iter()
        .filter_map(|value| non_empty_string(value))
        .collect::<Vec<_>>();
    let language = (hints.len() == 1).then(|| hints[0].clone());
    serde_json::to_value(SdkFileAsrInput {
        audio: json!({ "kind": "media-ref", "ref": "input-audio" }),
        language,
        hints,
        timestamps: true,
        options: SdkFileAsrOptions {
            context: non_empty_string(&customization.context),
            vocabulary_id,
            diarization_enabled: params.diarization_enabled,
            speaker_count: params.speaker_count,
            channel_id: params
                .channel_id
                .as_ref()
                .filter(|value| !value.is_null())
                .cloned(),
            special_word_filter: non_empty_string(&params.special_word_filter),
        },
    })
    .expect("SDK 文件识别输入只包含可序列化基础类型")
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
        if profile.id != scope.profile_id {
            return Err(format!(
                "内置 SDK scope {} 不能绑定供应商配置 {}",
                scope.provider_id, profile.id
            ));
        }
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
        source: &str,
        module_id: &str,
        input: Value,
        request_id: &str,
    ) -> Result<(), String> {
        self.runtime
            .call(
                "realtimeStart",
                &json!({
                    "source": source,
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

pub async fn recognize_sdk_file(
    profile: ProviderProfile,
    credentials: CredentialStoreHandle,
    path: String,
    params: crate::providers::alibabacloud::TranscriptionParams,
    customization: crate::providers::RequestCustomization,
    cancelled: Option<Arc<AtomicBool>>,
) -> Result<crate::providers::alibabacloud::TranscriptionResult, String> {
    let model = params.model_id();
    let route = crate::providers::registry::builtin_sdk_asr_route(&model)
        .filter(|route| !route.realtime)
        .ok_or_else(|| format!("模型 {model} 没有内置 SDK 文件识别路由"))?;
    if route.provider_id != profile.id {
        return Err(format!(
            "模型 {model} 属于供应商 {}，不能由 {} 执行",
            route.provider_id, profile.id
        ));
    }
    let scope = BuiltinSdkScope::speech_recognition(&profile)?;
    let request_id = format!("{}-asr-{}", scope.provider_id(), uuid::Uuid::new_v4());
    let vocabulary_id = profile
        .config
        .get("vocabularyIds")
        .and_then(|value| value.get(&model))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string);
    let input = sdk_file_asr_input(&params, &customization, vocabulary_id);
    let cancel = cancelled.unwrap_or_else(|| Arc::new(AtomicBool::new(false)));
    let value = crate::providers::plugin_runtime::spawn_js_worker("builtin-file-asr", move || {
        let runtime = BuiltinSdkRuntime::create(
            &profile,
            credentials,
            scope,
            request_id.clone(),
            cancel,
            HashMap::from([("input-audio".into(), PathBuf::from(path))]),
        )?;
        runtime.execute_capability_with_timeout(
            &route.source,
            &route.module_id,
            input,
            &request_id,
            FILE_ASR_TIMEOUT,
        )
    })?
    .await
    .map_err(|error| format!("内置 SDK 识别工作线程失败：{error}"))??;
    sdk_asr_to_legacy(value)
}

pub async fn translate_bailian<F>(
    profile: ProviderProfile,
    credentials: CredentialStoreHandle,
    model: String,
    text: String,
    source_language: String,
    target_language: String,
    cancellation: CancellationToken,
    mut on_delta: F,
) -> Result<String, String>
where
    F: FnMut(&str),
{
    if cancellation.is_cancelled() {
        return Err("翻译请求已取消".into());
    }
    let request_id = format!("bailian-translation-{}", uuid::Uuid::new_v4());
    let module_id = format!("bailian.translation.{model}");
    let input = json!({
        "source": text,
        "sourceLanguage": source_language,
        "targetLanguage": target_language,
        "options": { "stream": true },
    });
    let (event_tx, mut event_rx) = mpsc::channel(128);
    let cancelled = Arc::new(AtomicBool::new(false));
    let task_cancelled = cancelled.clone();
    let task_request_id = request_id.clone();
    let mut task =
        crate::providers::plugin_runtime::spawn_js_worker("builtin-translation", move || {
            let runtime = BuiltinSdkRuntime::create_with_event_sender(
                &profile,
                credentials,
                BuiltinSdkScope::BAILIAN_TRANSLATION,
                task_request_id.clone(),
                task_cancelled,
                HashMap::new(),
                Some(event_tx),
            )?;
            runtime.execute_capability("translation", &module_id, input, &task_request_id)
        })?;
    let mut accumulated = String::new();
    loop {
        tokio::select! {
            _ = cancellation.cancelled() => {
                cancelled.store(true, std::sync::atomic::Ordering::Release);
                let _ = task.await;
                return Err("翻译请求已取消".into());
            }
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

/// SDK 的毫秒时间戳契约是 `number`，供应商按秒返回时会得到小数毫秒（例如 Groq 的
/// 25049.99936）；本地契约用整数毫秒，因此统一四舍五入并钳到非负范围。
fn sdk_millis(value: Option<&Value>) -> Option<u64> {
    match value? {
        Value::Number(number) => {
            if let Some(value) = number.as_u64() {
                Some(value)
            } else if let Some(value) = number.as_i64() {
                Some(value.max(0) as u64)
            } else {
                let value = number.as_f64()?;
                if value.is_finite() {
                    Some(value.round().max(0.0) as u64)
                } else {
                    None
                }
            }
        }
        _ => None,
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
                        "beginTime": sdk_millis(word.get("startMs")).unwrap_or_default(),
                        "endTime": sdk_millis(word.get("endMs")).unwrap_or_default(),
                        "text": word.get("text").and_then(Value::as_str).unwrap_or_default(),
                    })
                })
                .collect::<Vec<_>>();
            json!({
                "beginTime": sdk_millis(segment.get("startMs")).unwrap_or_default(),
                "endTime": sdk_millis(segment.get("endMs")).unwrap_or_default(),
                "text": segment.get("text").and_then(Value::as_str).unwrap_or_default(),
                "words": words,
            })
        })
        .collect::<Vec<_>>();
    serde_json::from_value(json!({
        "durationMs": sdk_millis(value.get("durationMs")),
        "transcripts": [{
            "channelId": Value::Null,
            "text": value.get("text").and_then(Value::as_str).unwrap_or_default(),
            "sentences": sentences,
        }],
    }))
    .map_err(|error| format!("内置 SDK 识别结果映射失败：{error}"))
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
    let root = std::env::temp_dir().join("say-it-sdk-runtime-0.2.8");
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
    fn p0_asr_scopes_keep_provider_hosts_and_credential_owners_isolated() {
        let volcengine =
            BuiltinSdkScope::speech_recognition(&crate::providers::volcengine_profile()).unwrap();
        assert_eq!(volcengine.provider_id(), "volcengine");
        assert_eq!(volcengine.credential_owner(), "volcengine");
        assert_eq!(volcengine.allowed_hosts(), ["openspeech.bytedance.com"]);

        let siliconflow =
            BuiltinSdkScope::speech_recognition(&crate::providers::siliconflow_profile()).unwrap();
        assert_eq!(siliconflow.provider_id(), "siliconflow");
        assert_eq!(siliconflow.allowed_hosts(), ["api.siliconflow.cn"]);

        let groq =
            BuiltinSdkScope::speech_recognition(&crate::providers::groq_llm_profile()).unwrap();
        assert_eq!(groq.provider_id(), "groq");
        assert_eq!(
            groq.credential_owner(),
            crate::providers::GROQ_LLM_PROVIDER_ID
        );
        assert_eq!(groq.credential_scope(), "speech-recognition");
        assert_eq!(groq.allowed_hosts(), ["api.groq.com"]);
    }

    #[test]
    fn sdk_scope_rejects_a_different_provider_profile_before_runtime_creation() {
        let scope =
            BuiltinSdkScope::speech_recognition(&crate::providers::volcengine_profile()).unwrap();
        let error = BuiltinSdkRuntime::create(
            &crate::providers::bailian_profile(),
            CredentialStoreHandle::default(),
            scope,
            "mismatched-provider",
            Arc::new(AtomicBool::new(false)),
            HashMap::new(),
        )
        .err()
        .expect("不同供应商的 profile 与 SDK scope 必须被拒绝");
        assert!(error.contains("不能绑定供应商配置 bailian"));
    }

    #[tokio::test]
    async fn pre_cancelled_translation_stops_before_credentials_or_network() {
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        let result = translate_bailian(
            crate::providers::bailian_profile(),
            CredentialStoreHandle::default(),
            "qwen-mt-flash".into(),
            "待翻译".into(),
            "zh".into(),
            "en".into(),
            cancellation,
            |_| {},
        )
        .await;
        assert_eq!(result.unwrap_err(), "翻译请求已取消");
    }

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

    #[test]
    fn maps_fractional_sdk_milliseconds_by_rounding() {
        let result = sdk_asr_to_legacy(json!({
            "text": "hello",
            "durationMs": 25049.99936,
            "segments": [{
                "text": "hello",
                "startMs": 1234.5,
                "endMs": 2000.4,
                "words": [{ "text": "hello", "startMs": -0.2, "endMs": 999.6 }],
            }],
        }))
        .unwrap();
        assert_eq!(result.duration_ms, Some(25050));
        let sentence = &result.transcripts[0].sentences[0];
        assert_eq!(sentence.begin_time, 1235);
        assert_eq!(sentence.end_time, 2000);
        assert_eq!(sentence.words[0].begin_time, 0);
        assert_eq!(sentence.words[0].end_time, 1000);
    }

    #[test]
    fn sdk_file_asr_input_omits_absent_optional_fields_instead_of_serializing_null() {
        let mut params = crate::providers::alibabacloud::TranscriptionParams::default();
        params.channel_id = Some(Value::Null);
        let input = sdk_file_asr_input(
            &params,
            &crate::providers::RequestCustomization::default(),
            None,
        );
        assert_eq!(
            input,
            json!({
                "audio": { "kind": "media-ref", "ref": "input-audio" },
                "timestamps": true,
            })
        );
        assert!(!input.to_string().contains("null"));
    }

    #[test]
    fn sdk_file_asr_input_keeps_valid_language_hints_and_provider_options() {
        let params = crate::providers::alibabacloud::TranscriptionParams {
            language_hints: vec![" zh ".into(), "en".into()],
            diarization_enabled: Some(true),
            speaker_count: Some(2),
            channel_id: Some(json!(1)),
            special_word_filter: " names ".into(),
            ..Default::default()
        };
        let customization = crate::providers::RequestCustomization {
            context: " product terms ".into(),
            ..Default::default()
        };
        let input = sdk_file_asr_input(&params, &customization, Some("vocabulary-1".into()));
        assert!(input.get("language").is_none());
        assert_eq!(input["hints"], json!(["zh", "en"]));
        assert_eq!(
            input["options"],
            json!({
                "context": "product terms",
                "vocabularyId": "vocabulary-1",
                "diarizationEnabled": true,
                "speakerCount": 2,
                "channelId": 1,
                "specialWordFilter": "names",
            })
        );

        let single_language = sdk_file_asr_input(
            &crate::providers::alibabacloud::TranscriptionParams {
                language_hints: vec![" zh ".into()],
                ..Default::default()
            },
            &crate::providers::RequestCustomization::default(),
            None,
        );
        assert_eq!(single_language["language"], "zh");
        assert_eq!(single_language["hints"], json!(["zh"]));
    }
}
