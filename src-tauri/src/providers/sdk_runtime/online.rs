use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use crate::cancellation::CancellationFlag;
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
const HOST_TIMEOUT_MARGIN: Duration = Duration::from_secs(5);

/// 一个内置 SDK 运行时能用的密钥与能访问的主机。密钥按供应商配置 ID（`profile_id`）存放。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuiltinSdkScope {
    profile_id: String,
    provider_id: String,
    credential_field: String,
    credential_scope: &'static str,
    allowed_hosts: Vec<String>,
    /// 预连接的地址（协议、主机、端口）；通配主机事先不知道具体域名，不在其中。
    prewarm_origins: Vec<String>,
    /// 用户自己填的本机 http 地址（如本地模型服务）。
    local_network: bool,
}

impl BuiltinSdkScope {
    fn new(
        profile_id: &str,
        provider_id: &str,
        credential_field: &str,
        credential_scope: &'static str,
        allowed_hosts: &[&str],
    ) -> Self {
        Self {
            profile_id: profile_id.into(),
            provider_id: provider_id.into(),
            credential_field: credential_field.into(),
            credential_scope,
            allowed_hosts: allowed_hosts.iter().map(|host| (*host).into()).collect(),
            prewarm_origins: allowed_hosts
                .iter()
                .filter(|host| !host.contains('*'))
                .map(|host| format!("https://{host}/"))
                .collect(),
            local_network: false,
        }
    }

    pub fn bailian_translation() -> Self {
        Self::new("bailian", "bailian", "apiKey", "translation", &["*.aliyuncs.com"])
    }

    pub fn speech_recognition(profile: &ProviderProfile) -> Result<Self, String> {
        match profile.kind.as_str() {
            "sdk:bailian" => Ok(Self::new("bailian", "bailian", "apiKey", "speech-recognition", &["*.aliyuncs.com"])),
            "sdk:volcengine" => Ok(Self::new(
                "volcengine",
                "volcengine",
                "apiKey",
                "speech-recognition",
                &["openspeech.bytedance.com"],
            )),
            "sdk:siliconflow" => Ok(Self::new(
                "siliconflow",
                "siliconflow",
                "apiKey",
                "speech-recognition",
                &["api.siliconflow.cn"],
            )),
            "llm:groq" => Ok(Self::new(
                crate::providers::GROQ_LLM_PROVIDER_ID,
                "groq",
                "apiKey",
                "speech-recognition",
                &["api.groq.com"],
            )),
            _ => Err(format!(
                "供应商 {} 没有内置 SDK 语音识别运行时",
                profile.display_name
            )),
        }
    }

    /// 大模型请求：`provider_id` 是 SDK 供应商 ID。SDK 登记过的供应商只能访问 SDK 为它解析出的主机；
    /// 给了地址的（自定义地址等）只能访问这个地址的主机，明文 http 只放行本机。
    pub fn llm(
        profile: &ProviderProfile,
        provider_id: &str,
        base_url: Option<&str>,
    ) -> Result<Self, String> {
        let mut scope = Self::new(&profile.id, provider_id, "", "llm", &[]);
        scope.credential_field = crate::providers::llm_api_key_field(profile);
        match (base_url, super::SDK_LLM_HOSTS.get(provider_id)) {
            (Some(base_url), _) => {
                let url = url::Url::parse(base_url)
                    .ok()
                    .filter(|url| url.host_str().is_some())
                    .ok_or_else(|| format!("{} 的接口地址无效", profile.display_name))?;
                scope.allowed_hosts = vec![url.host_str().unwrap_or_default().to_string()];
                scope.prewarm_origins = vec![format!("{}/", url.origin().ascii_serialization())];
                scope.local_network = url.scheme() == "http";
            }
            (None, Some(hosts)) => {
                scope.prewarm_origins = hosts.iter().map(|host| format!("https://{host}/")).collect();
                scope.allowed_hosts = hosts.clone();
            }
            (None, None) => return Err(format!("{} 需要填写接口地址", profile.display_name)),
        }
        Ok(scope)
    }

    fn provider_id(&self) -> &str {
        &self.provider_id
    }

    /// 预连接这个运行时会访问的地址，正式请求复用连接。
    pub async fn prewarm(&self) -> Result<(), String> {
        for origin in &self.prewarm_origins {
            crate::providers::plugin_runtime::prewarm_connection(origin).await?;
        }
        Ok(())
    }

    fn allowed_hosts(&self) -> Vec<String> {
        self.allowed_hosts.clone()
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
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    vocabulary: std::collections::BTreeMap<String, i32>,
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
        self.vocabulary.is_empty()
            && self.context.is_none()
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

/// 新模型的即时热词直接使用全局词表；旧模型继续使用已同步的 vocabularyId。
pub(crate) fn sdk_inline_vocabulary(
    model: &str,
    customization: &crate::providers::RequestCustomization,
) -> std::collections::BTreeMap<String, i32> {
    if !matches!(model,
        "qwen-audio-3.1-asr-flash"
        | "qwen-audio-3.1-asr-flash-filetrans"
        | "qwen-audio-3.1-asr-flash-streaming"
    ) {
        return Default::default();
    }
    customization.hotwords.iter()
        .filter_map(|word| non_empty_string(&word.text).map(|text| (text, word.weight.clamp(1, 5))))
        .collect()
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
            vocabulary: sdk_inline_vocabulary(&params.model_id(), customization),
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
        cancelled: Arc<CancellationFlag>,
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
        cancelled: Arc<CancellationFlag>,
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
        cancelled: Arc<CancellationFlag>,
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
        let credential_key = CredentialKey::provider(&scope.profile_id, &scope.credential_field)?;
        let bindings = SdkHostBindings {
            owner_id: format!("builtin:{}", scope.provider_id()),
            provider_id: scope.provider_id().into(),
            request_id,
            credential_scopes: HashSet::from([scope.credential_scope.into()]),
            credential_key,
            credentials,
            recorder,
        };
        let runtime = JsProviderRuntime::create_with_sdk_bindings_and_event_sender(
            runtime_spec(&scope)?,
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
        cancelled: Arc<CancellationFlag>,
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

    /// 执行一次大模型对话；`request` 是 SDK 的 `LlmChatRequestDto`。`emit_events` 时逐段推送正文与思考。
    /// 超时由 SDK 计时并报错，宿主多等一会儿只兜住 SDK 本身卡住的情况。
    pub fn run_llm(
        &self,
        request: Value,
        request_id: &str,
        emit_events: bool,
        timeout: Duration,
    ) -> Result<Value, String> {
        self.runtime.call(
            "invoke",
            &json!({
                "operation": "llm.run",
                "input": request,
                "requestId": request_id,
                "timeoutMs": timeout.as_millis(),
                "emitEvents": emit_events,
                // SDK 目录里没有的模型（手动添加、自定义地址）能力未知；用户给它选了思考程度，
                // 按支持思考声明，SDK 才会按供应商写法发出开启或关闭。
                "fallbackCapabilities": { "reasoning": true },
            }),
            timeout + HOST_TIMEOUT_MARGIN,
        )
    }

    /// 列出供应商可用模型；`base_url` 为空时按 SDK 预设地址。
    pub fn discover_llm(
        &self,
        provider_id: &str,
        base_url: Option<&str>,
        timeout: Duration,
    ) -> Result<Value, String> {
        let mut request = json!({
            "operation": "llm.discover",
            "providerId": provider_id,
            "timeoutMs": timeout.as_millis(),
        });
        if let Some(base_url) = base_url {
            request["baseUrl"] = json!(base_url);
        }
        self.runtime.call("invoke", &request, timeout + HOST_TIMEOUT_MARGIN)
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

    pub(crate) fn host_events_ready(&self) -> &tokio::sync::Notify {
        self.runtime.host_events_ready()
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
    cancelled: Option<Arc<CancellationFlag>>,
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
    let cancel = cancelled.unwrap_or_else(|| Arc::new(CancellationFlag::new(false)));
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
    let cancelled = Arc::new(CancellationFlag::new(false));
    let task_cancelled = cancelled.clone();
    let _cancel_on_drop = cancelled.cancel_on_drop();
    let task_request_id = request_id.clone();
    let mut task =
        crate::providers::plugin_runtime::spawn_js_worker("builtin-translation", move || {
            let runtime = BuiltinSdkRuntime::create_with_event_sender(
                &profile,
                credentials,
                BuiltinSdkScope::bailian_translation(),
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
                "speakerId": segment.get("speakerId"),
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

fn runtime_spec(scope: &BuiltinSdkScope) -> Result<PluginRuntimeSpec, String> {
    let root = builtin_root()?;
    Ok(PluginRuntimeSpec {
        plugin_id: format!("builtin-sdk-{}", scope.provider_id()),
        source_namespace: "@henjicc/ai-sdk".into(),
        capabilities: vec![],
        secret_fields: vec![scope.credential_field.clone()],
        credentials: None,
        root: root.clone(),
        entrypoint: root.join("connector/index.js"),
        permissions: if scope.local_network {
            vec!["network".into(), "localNetwork".into()]
        } else {
            vec!["network".into()]
        },
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
    let root = std::env::temp_dir().join("say-it-sdk-runtime-0.15.0");
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
        assert_eq!(volcengine.profile_id, "volcengine");
        assert_eq!(volcengine.allowed_hosts(), ["openspeech.bytedance.com"]);

        let siliconflow =
            BuiltinSdkScope::speech_recognition(&crate::providers::siliconflow_profile()).unwrap();
        assert_eq!(siliconflow.provider_id(), "siliconflow");
        assert_eq!(siliconflow.allowed_hosts(), ["api.siliconflow.cn"]);

        let groq =
            BuiltinSdkScope::speech_recognition(&crate::providers::groq_llm_profile()).unwrap();
        assert_eq!(groq.provider_id(), "groq");
        assert_eq!(groq.profile_id, crate::providers::GROQ_LLM_PROVIDER_ID);
        assert_eq!(groq.credential_scope, "speech-recognition");
        assert_eq!(groq.allowed_hosts(), ["api.groq.com"]);
    }

    /// 大模型运行时只能访问 SDK 为该供应商解析出的主机；自定义地址只能访问它自己。
    #[test]
    fn llm_scopes_use_sdk_hosts_and_the_llm_key_field() {
        let volcengine = crate::providers::volcengine_profile();
        let scope = BuiltinSdkScope::llm(&volcengine, "volcengine", None).unwrap();
        assert_eq!(scope.allowed_hosts(), ["ark.cn-beijing.volces.com"]);
        assert_eq!(scope.credential_field, crate::providers::llm_api_key_field(&volcengine));
        assert_ne!(scope.credential_field, "apiKey", "火山引擎的大模型用方舟密钥");

        let groq = crate::providers::groq_llm_profile();
        let scope =
            BuiltinSdkScope::llm(&groq, "custom", Some("https://llm.example.com:8443/v1")).unwrap();
        assert_eq!(scope.allowed_hosts(), ["llm.example.com"]);
        assert_eq!(scope.prewarm_origins, ["https://llm.example.com:8443/"]);
        assert!(!scope.local_network);
        assert!(BuiltinSdkScope::llm(&groq, "custom", None).is_err());

        let local = BuiltinSdkScope::llm(&groq, "custom", Some("http://127.0.0.1:11434/v1")).unwrap();
        assert!(local.local_network, "本机模型服务用明文 http");
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
            Arc::new(CancellationFlag::new(false)),
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
                "speakerId": 2,
                "words": [{ "text": "你", "startMs": 10, "endMs": 300 }],
            }],
        }))
        .unwrap();
        assert_eq!(result.duration_ms, Some(1200));
        assert_eq!(result.transcripts[0].text, "你好");
        assert_eq!(result.transcripts[0].sentences[0].begin_time, 10);
        assert_eq!(result.transcripts[0].sentences[0].speaker_id, Some(json!(2)));
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
    fn qwen_audio_file_receives_inline_hotwords_without_changing_old_models() {
        let customization = crate::providers::RequestCustomization {
            hotwords: vec![crate::providers::alibabacloud::HotwordEntry { text: " 说吧 ".into(), weight: 3 }],
            ..Default::default()
        };
        for model in ["qwen-audio-3.1-asr-flash", "qwen-audio-3.1-asr-flash-filetrans"] {
            let params = crate::providers::alibabacloud::TranscriptionParams { model: model.into(), ..Default::default() };
            let input = sdk_file_asr_input(&params, &customization, None);
            assert_eq!(input["options"]["vocabulary"], json!({"说吧":3}));
            assert!(input["options"].get("vocabularyId").is_none());
        }
        assert!(sdk_inline_vocabulary("fun-asr", &customization).is_empty());
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

    /// 回归：SDK 的 `openSession` 把 `timeoutMs` 当作「从 open() 起整段会话的强制
    /// 超时」，内部直接 `setTimeout(abort, timeoutMs)` 且不随 send/finish 续期。
    /// 而 `DEFAULT_TIMEOUT` 是**单次请求**预算（45 秒），一旦转发进去，任何超过
    /// 该时长的实时听写/实时字幕都会被硬切断。插件路径（plugin_runtime.rs 的
    /// `__sayitPluginCapabilityOpen`）早已刻意不转发，内置 SDK 这条路径曾漏掉。
    ///
    /// 这里对 `include_str!` 进来的入口源码做契约检查：非实时路径仍应带
    /// `timeoutMs`（那是正确的单次请求超时），唯独 `realtimeStart` 不得带。
    #[test]
    fn realtime_start_must_not_forward_timeout_into_open_session() {
        let start = BUILTIN_SOURCE
            .find("async realtimeStart(")
            .expect("入口应当定义 realtimeStart");
        let rest = &BUILTIN_SOURCE[start + "async realtimeStart(".len()..];
        let end = rest.find("async realtime").unwrap_or(rest.len());
        let body = &rest[..end];

        assert!(
            body.contains("openSession"),
            "realtimeStart 应当通过 openSession 打开会话"
        );
        // 匹配属性赋值而不是单词本身——realtimeStart 里有一段解释为何不转发的注释，
        // 那段注释提到了 request.timeoutMs，不应被误判成转发。
        assert!(
            !body.contains("timeoutMs:"),
            "realtimeStart 不得把 timeoutMs 转发进 openSession：             那会让实时会话在固定时长后被硬切断"
        );
        assert!(
            BUILTIN_SOURCE.contains("timeoutMs: request.timeoutMs"),
            "非实时路径仍应保留单次请求超时，否则本测试已失去意义"
        );
    }
}
