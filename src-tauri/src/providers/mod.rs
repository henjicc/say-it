use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub mod alibabacloud;
pub mod apple_speech;
pub mod browser_session_capture;
pub mod capabilities;
pub mod credential_store;
pub(crate) mod credential_vault;
pub mod local_asr;
pub mod model_download;
pub mod plugin;
pub mod plugin_capability;
pub mod plugin_package;
pub mod plugin_runtime;
pub mod plugin_secrets;
pub mod registry;
pub mod sdk_runtime;
#[cfg(test)]
mod testing;

/// 一次识别请求携带的定制数据。内容由应用层的全局热词与上下文渲染得到
/// （见 `application::customization`），供应商层只负责按模型声明的能力下发：
/// `supportsVocabulary` 的模型收到 `hotwords`，`supportsContext` 的模型收到 `context`。
#[derive(Clone, Debug, Default)]
pub struct RequestCustomization {
    pub hotwords: Vec<alibabacloud::HotwordEntry>,
    pub context: String,
}

impl RequestCustomization {
    /// 把定制数据写进插件调用载荷。空字段不写：插件只在宿主真的有内容下发时才看到
    /// `hotwords` / `context`，可以据此区分"用户没配"和"模型不支持"。
    pub fn write_into(&self, payload: &mut serde_json::Map<String, Value>) {
        if !self.hotwords.is_empty() {
            if let Ok(value) = serde_json::to_value(&self.hotwords) {
                payload.insert("hotwords".into(), value);
            }
        }
        if !self.context.trim().is_empty() {
            payload.insert("context".into(), json!(self.context));
        }
    }
}

pub const BAILIAN_PROVIDER_ID: &str = "bailian";
pub const VOLCENGINE_PROVIDER_ID: &str = "volcengine";
pub const SILICONFLOW_PROVIDER_ID: &str = "siliconflow";
pub const GROQ_LLM_PROVIDER_ID: &str = "llm-groq";
pub const SYSTEM_OCR_PROVIDER_ID: &str = "system-ocr";
pub const DEFAULT_LLM_TEMPERATURE: f64 = 0.1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmModelConfig {
    pub name: String,
    #[serde(default)]
    pub source: LlmModelSource,
    #[serde(default)]
    pub availability: LlmModelAvailability,
    #[serde(default = "default_reasoning_effort")]
    pub reasoning_effort: String,
    #[serde(default = "default_llm_temperature")]
    pub temperature: Option<f64>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
}

impl LlmModelConfig {
    pub fn manual(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            source: LlmModelSource::Manual,
            availability: LlmModelAvailability::Unknown,
            reasoning_effort: default_reasoning_effort(),
            temperature: default_llm_temperature(),
            max_tokens: None,
        }
    }

    pub fn remote(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            source: LlmModelSource::Remote,
            availability: LlmModelAvailability::Available,
            reasoning_effort: default_reasoning_effort(),
            temperature: default_llm_temperature(),
            max_tokens: None,
        }
    }

    pub fn has_custom_options(&self) -> bool {
        self.reasoning_effort != "auto"
            || self.temperature != Some(DEFAULT_LLM_TEMPERATURE)
            || self.max_tokens.is_some()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LlmModelSource {
    Remote,
    #[default]
    Manual,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LlmModelAvailability {
    Available,
    Missing,
    #[default]
    Unknown,
}

fn default_reasoning_effort() -> String {
    "auto".to_string()
}

fn default_llm_temperature() -> Option<f64> {
    Some(DEFAULT_LLM_TEMPERATURE)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderProfile {
    pub id: String,
    pub kind: String,
    pub display_name: String,
    pub auth_kind: String,
    pub capabilities: Vec<String>,
    pub enabled: bool,
    #[serde(default)]
    pub config: Value,
    #[serde(default)]
    pub config_fields: Vec<ProviderConfigField>,
    #[serde(default)]
    pub actions: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderDefaults {
    pub asr: String,
    /// 预留给 LLM 后处理能力的默认供应商；空串表示未设置。旧 JSON 没有这个字段，靠 `#[serde(default)]` 兼容。
    #[serde(default)]
    pub llm: String,
    #[serde(default)]
    pub translation: String,
    /// OCR 能力默认供应商；空串表示未设置，normalize 后落到内置系统 OCR。旧 JSON 靠 `#[serde(default)]` 兼容。
    #[serde(default)]
    pub ocr: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSettings {
    pub profiles: Vec<ProviderProfile>,
    pub defaults: ProviderDefaults,
}

impl Default for ProviderSettings {
    fn default() -> Self {
        Self {
            profiles: builtin_profiles(),
            defaults: ProviderDefaults {
                asr: BAILIAN_PROVIDER_ID.to_string(),
                llm: GROQ_LLM_PROVIDER_ID.to_string(),
                translation: BAILIAN_PROVIDER_ID.to_string(),
                ocr: SYSTEM_OCR_PROVIDER_ID.to_string(),
            },
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderStatus {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub has_api_key: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configured: Option<bool>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderListItem {
    pub id: String,
    pub kind: String,
    pub display_name: String,
    pub auth_kind: String,
    pub capabilities: Vec<String>,
    pub enabled: bool,
    pub is_default_asr: bool,
    pub effective_capabilities: Vec<String>,
    pub config_fields: Vec<ProviderConfigField>,
    pub actions: Vec<String>,
    pub status: Option<ProviderStatus>,
    /// 非密钥配置（如热词、语种提示等），用于前端回显；apiKey 等密钥字段会被剔除。
    pub config: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderConfigField {
    pub key: String,
    pub label: String,
    pub field_type: String,
    pub secret: bool,
}

pub(crate) fn is_sensitive_config_key(key: &str) -> bool {
    let normalized = key
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect::<String>();
    matches!(
        normalized.as_str(),
        "authorization"
            | "cookie"
            | "credential"
            | "password"
            | "passwd"
            | "secret"
            | "accesskeysecret"
    ) || normalized.ends_with("apikey")
        || normalized.ends_with("password")
        || normalized.ends_with("privatekey")
        || normalized.ends_with("secret")
        || normalized.ends_with("token")
}

fn is_host_secret_field(auth_kind: &str, field: &ProviderConfigField) -> bool {
    field.secret
        || field.field_type.eq_ignore_ascii_case("password")
        || is_sensitive_config_key(&field.key)
        || (auth_kind == "api-key" && field.key.eq_ignore_ascii_case("apiKey"))
}

pub(crate) fn secret_config_keys(profile: &ProviderProfile) -> std::collections::HashSet<String> {
    let mut keys = profile
        .config_fields
        .iter()
        .filter(|field| is_host_secret_field(&profile.auth_kind, field))
        .map(|field| field.key.clone())
        .collect::<std::collections::HashSet<_>>();
    if profile.auth_kind == "api-key" {
        keys.insert("apiKey".into());
    }
    if let Some(config) = profile.config.as_object() {
        keys.extend(
            config
                .keys()
                .filter(|key| is_sensitive_config_key(key))
                .cloned(),
        );
    }
    keys
}

pub fn config_fields_for(profile: &ProviderProfile) -> Vec<ProviderConfigField> {
    let mut fields = if !profile.config_fields.is_empty() {
        profile.config_fields.clone()
    } else {
        match profile.kind.as_str() {
            "sdk:bailian" => vec![ProviderConfigField {
                key: "apiKey".into(),
                label: "API Key".into(),
                field_type: "password".into(),
                secret: true,
            }],
            "sdk:volcengine" => vec![ProviderConfigField {
                key: "apiKey".into(),
                label: "APP Key".into(),
                field_type: "password".into(),
                secret: true,
            }],
            "llm:groq" => vec![ProviderConfigField {
                key: "apiKey".into(),
                label: "API Key".into(),
                field_type: "password".into(),
                secret: true,
            }],
            kind if kind.starts_with("llm:") => vec![
                ProviderConfigField {
                    key: "apiKey".into(),
                    label: "API Key".into(),
                    field_type: "password".into(),
                    secret: true,
                },
                ProviderConfigField {
                    key: "model".into(),
                    label: "模型".into(),
                    field_type: "text".into(),
                    secret: false,
                },
            ],
            _ if profile.auth_kind == "api-key" => vec![ProviderConfigField {
                key: "apiKey".into(),
                label: "API Key".into(),
                field_type: "password".into(),
                secret: true,
            }],
            _ => Vec::new(),
        }
    };
    for field in &mut fields {
        field.secret = is_host_secret_field(&profile.auth_kind, field);
    }
    if profile.auth_kind == "api-key" && !fields.iter().any(|field| field.secret) {
        fields.push(ProviderConfigField {
            key: "apiKey".into(),
            label: "API Key".into(),
            field_type: "password".into(),
            secret: true,
        });
    }
    fields
}

pub fn actions_for(profile: &ProviderProfile) -> Vec<String> {
    if !profile.actions.is_empty() {
        return profile.actions.clone();
    }
    match profile.kind.as_str() {
        "sdk:bailian" => vec!["manageHotwords".into(), "testRealtimeAsr".into()],
        _ => Vec::new(),
    }
}

pub fn sanitized_config(profile: &ProviderProfile) -> Value {
    let mut sanitized = profile.config.clone();
    if let Some(obj) = sanitized.as_object_mut() {
        for field in secret_config_keys(profile) {
            obj.remove(&field);
        }
    }
    sanitized
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSettingsResponse {
    pub profiles: Vec<ProviderListItem>,
    pub defaults: ProviderDefaults,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetDefaultProviderRequest {
    pub capability: String,
    pub provider_id: String,
}

pub fn bailian_profile() -> ProviderProfile {
    ProviderProfile {
        id: BAILIAN_PROVIDER_ID.to_string(),
        kind: "sdk:bailian".to_string(),
        display_name: "阿里云百炼".to_string(),
        auth_kind: "api-key".to_string(),
        // 同一把百炼 Key 同时供 ASR 识别与 Qwen-MT 翻译（llm 能力）使用，不新增独立供应商。
        capabilities: vec![
            "asr".to_string(),
            "llm".to_string(),
            "translation".to_string(),
        ],
        enabled: true,
        config: json!({
            "vocabularyIds": {},
            "languageHints": [],
            "semanticPunctuationEnabled": false,
            "maxSentenceSilence": 1300,
            "multiThresholdModeEnabled": false,
            "heartbeat": false,
            "speechNoiseThreshold": null
        }),
        config_fields: vec![],
        actions: vec![],
    }
}

pub fn volcengine_profile() -> ProviderProfile {
    ProviderProfile {
        id: VOLCENGINE_PROVIDER_ID.to_string(),
        kind: "sdk:volcengine".to_string(),
        display_name: "火山引擎".to_string(),
        auth_kind: "api-key".to_string(),
        capabilities: vec!["asr".to_string()],
        enabled: true,
        config: json!({}),
        config_fields: vec![],
        actions: vec![],
    }
}

pub fn siliconflow_profile() -> ProviderProfile {
    ProviderProfile {
        id: SILICONFLOW_PROVIDER_ID.to_string(),
        kind: "sdk:siliconflow".to_string(),
        display_name: "硅基流动".to_string(),
        auth_kind: "api-key".to_string(),
        capabilities: vec!["asr".to_string()],
        enabled: true,
        config: json!({}),
        config_fields: vec![],
        actions: vec![],
    }
}

pub fn groq_llm_profile() -> ProviderProfile {
    ProviderProfile {
        id: GROQ_LLM_PROVIDER_ID.to_string(),
        kind: "llm:groq".to_string(),
        display_name: "Groq".to_string(),
        auth_kind: "api-key".to_string(),
        // Groq 的 LLM 与 Whisper ASR 复用同一份供应商配置和加密凭据。
        capabilities: vec!["llm".to_string(), "asr".to_string()],
        enabled: true,
        config: json!({
            "model": "openai/gpt-oss-20b",
            "models": [LlmModelConfig::manual("openai/gpt-oss-20b")]
        }),
        config_fields: vec![],
        actions: vec![],
    }
}

pub fn system_ocr_display_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "macOS 系统 OCR"
    } else {
        "Windows 系统 OCR"
    }
}

fn system_ocr_kind() -> &'static str {
    if cfg!(target_os = "macos") {
        "builtin-macos-vision-ocr"
    } else {
        "builtin-windows-ocr"
    }
}

/// 内置系统 OCR：Windows 使用 WinRT，macOS 使用 Vision，无用户配置项。
pub fn system_ocr_profile() -> ProviderProfile {
    ProviderProfile {
        id: SYSTEM_OCR_PROVIDER_ID.to_string(),
        kind: system_ocr_kind().to_string(),
        display_name: system_ocr_display_name().to_string(),
        auth_kind: "none".to_string(),
        capabilities: vec!["ocr".to_string()],
        enabled: true,
        config: json!({}),
        config_fields: vec![],
        actions: vec![],
    }
}

pub fn apple_speech_profile() -> ProviderProfile {
    ProviderProfile {
        id: apple_speech::PROVIDER_ID.to_string(),
        kind: apple_speech::PROVIDER_KIND.to_string(),
        display_name: "Apple 系统本地识别".to_string(),
        auth_kind: "none".to_string(),
        capabilities: vec!["asr".to_string()],
        enabled: true,
        config: json!({}),
        config_fields: vec![],
        actions: vec![],
    }
}

pub fn find_profile<'a>(settings: &'a ProviderSettings, id: &str) -> Option<&'a ProviderProfile> {
    settings.profiles.iter().find(|profile| profile.id == id)
}

/// 删除供应商配置时只移除 profile；本地加密凭据按产品语义保留，便于重新安装插件或恢复配置。
/// 凭据只允许由未来明确的“忘记凭据”操作删除，卸载/移除供应商不得隐式清理。
pub fn remove_profile_preserving_credentials(settings: &mut ProviderSettings, id: &str) {
    settings.profiles.retain(|profile| profile.id != id);
}

/// 内置供应商清单：新增供应商时在这里追加一个 profile 构造函数。
pub fn builtin_profiles() -> Vec<ProviderProfile> {
    vec![
        bailian_profile(),
        volcengine_profile(),
        siliconflow_profile(),
        groq_llm_profile(),
        system_ocr_profile(),
        apple_speech_profile(),
    ]
}

pub fn llm_models_from_config(config: &Value) -> Vec<LlmModelConfig> {
    let mut models = config
        .get("models")
        .cloned()
        .and_then(|value| serde_json::from_value::<Vec<LlmModelConfig>>(value).ok())
        .unwrap_or_default();
    models.retain(|model| !model.name.trim().is_empty());
    for model in &mut models {
        model.name = model.name.trim().to_string();
    }
    let mut unique = Vec::with_capacity(models.len());
    for model in models {
        if !unique
            .iter()
            .any(|item: &LlmModelConfig| item.name == model.name)
        {
            unique.push(model);
        }
    }
    let current = config
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if !current.is_empty() && !unique.iter().any(|item| item.name == current) {
        unique.push(LlmModelConfig::manual(current));
    }
    unique
}

pub fn set_llm_models(config: &mut Value, models: &[LlmModelConfig]) -> Result<(), String> {
    let target = config
        .as_object_mut()
        .ok_or_else(|| "大语言模型配置格式异常".to_string())?;
    target.insert(
        "models".to_string(),
        serde_json::to_value(models).map_err(|error| error.to_string())?,
    );
    Ok(())
}

pub fn normalize_llm_endpoint(endpoint: &str) -> String {
    let endpoint = endpoint.trim();
    if endpoint.ends_with('/') {
        endpoint.to_string()
    } else {
        format!("{endpoint}/")
    }
}

/// 需要走 OpenAI Responses API、但不能复用 genai 默认端点的供应商。
/// 端点只作为协议路由默认值；用户在自定义配置中填写的 endpoint 仍优先。
pub fn llm_responses_endpoint(adapter: &str) -> Option<&'static str> {
    match adapter {
        "volcengine" => Some("https://ark.cn-beijing.volces.com/api/v3/"),
        "deepseek" => Some("https://api.deepseek.com/v1/"),
        "bailian" => Some("https://dashscope.aliyuncs.com/compatible-mode/v1/"),
        _ => None,
    }
}

pub fn llm_uses_responses(adapter: &str) -> bool {
    llm_responses_endpoint(adapter).is_some()
}

fn normalize_llm_profile_config(profile: &mut ProviderProfile) {
    if !profile.capabilities.iter().any(|value| value == "llm") {
        return;
    }
    if !profile.config.is_object() {
        profile.config = json!({});
    }
    let models = llm_models_from_config(&profile.config);
    let _ = set_llm_models(&mut profile.config, &models);
}

pub fn normalize_settings(mut settings: ProviderSettings) -> ProviderSettings {
    let migrate_legacy_llm_default =
        settings.defaults.llm.is_empty() || settings.defaults.llm == BAILIAN_PROVIDER_ID;
    for builtin in builtin_profiles() {
        match settings.profiles.iter_mut().find(|p| p.id == builtin.id) {
            Some(existing) => {
                // 只修正内置供应商的固定字段，非密钥 config 保留用户已保存的值。
                existing.kind = builtin.kind;
                existing.display_name = builtin.display_name;
                existing.auth_kind = builtin.auth_kind;
                existing.capabilities = builtin.capabilities;
                existing.config_fields = builtin.config_fields;
                existing.actions = builtin.actions;
                // enabled 现状仍强制为 true：UI 尚无停用开关，先维持现状。
                existing.enabled = true;
            }
            None => settings.profiles.push(builtin),
        }
    }
    if migrate_legacy_llm_default {
        settings.defaults.llm = GROQ_LLM_PROVIDER_ID.to_string();
    }
    for profile in &mut settings.profiles {
        normalize_llm_profile_config(profile);
    }
    // 未知 id 的 profile（用户手工配置或未来供应商）原样保留，不再删除。

    settings.defaults.asr = valid_or_fallback(&settings, &settings.defaults.asr, "asr");
    settings.defaults.llm = valid_or_fallback(&settings, &settings.defaults.llm, "llm");
    settings.defaults.translation =
        valid_or_fallback(&settings, &settings.defaults.translation, "translation");
    settings.defaults.ocr = valid_or_fallback(&settings, &settings.defaults.ocr, "ocr");

    settings
}

fn valid_or_fallback(settings: &ProviderSettings, provider_id: &str, capability: &str) -> String {
    if has_capability(settings, provider_id, capability) {
        provider_id.to_string()
    } else {
        fallback_provider_for(settings, capability)
    }
}

fn fallback_provider_for(settings: &ProviderSettings, capability: &str) -> String {
    if capability == "llm" && has_capability(settings, GROQ_LLM_PROVIDER_ID, capability) {
        return GROQ_LLM_PROVIDER_ID.to_string();
    }
    settings
        .profiles
        .iter()
        .find(|profile| {
            profile.enabled && profile.capabilities.iter().any(|item| item == capability)
        })
        .map(|profile| profile.id.clone())
        .unwrap_or_default()
}

pub fn has_capability(settings: &ProviderSettings, provider_id: &str, capability: &str) -> bool {
    settings.profiles.iter().any(|profile| {
        profile.enabled
            && profile.id == provider_id
            && profile.capabilities.iter().any(|item| item == capability)
    })
}

pub fn default_provider_id(settings: &ProviderSettings, capability: &str) -> String {
    match capability {
        "asr" => settings.defaults.asr.clone(),
        "llm" => settings.defaults.llm.clone(),
        "translation" => settings.defaults.translation.clone(),
        "ocr" => settings.defaults.ocr.clone(),
        _ => String::new(),
    }
}

pub fn set_default_provider(
    settings: &mut ProviderSettings,
    capability: &str,
    provider_id: &str,
) -> Result<(), String> {
    if !has_capability(settings, provider_id, capability) {
        return Err(format!("供应商 {provider_id} 不支持 {capability}"));
    }
    match capability {
        "asr" => settings.defaults.asr = provider_id.to_string(),
        "llm" => settings.defaults.llm = provider_id.to_string(),
        "translation" => settings.defaults.translation = provider_id.to_string(),
        "ocr" => settings.defaults.ocr = provider_id.to_string(),
        _ => return Err(format!("不支持的能力类型：{capability}")),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEGACY_STATE_JSON: &str = r#"{
        "profiles": [
            {
                "id": "funasr",
                "kind": "alibabacloud-funasr",
                "displayName": "阿里云百炼",
                "authKind": "api-key",
                "capabilities": ["asr"],
                "enabled": true,
                "config": {
                    "apiKey": "sk-legacy-key",
                    "hotwords": [{"text": "说吧", "weight": 3}],
                    "vocabularyIds": {"fun-asr-realtime": "vocab-123"}
                }
            }
        ],
        "defaults": {"asr": "funasr"}
    }"#;

    #[test]
    fn system_ocr_profile_matches_current_platform() {
        let profile = system_ocr_profile();
        if cfg!(target_os = "macos") {
            assert_eq!(profile.display_name, "macOS 系统 OCR");
            assert_eq!(profile.kind, "builtin-macos-vision-ocr");
        } else {
            assert_eq!(profile.display_name, "Windows 系统 OCR");
            assert_eq!(profile.kind, "builtin-windows-ocr");
        }
    }

    #[test]
    fn apple_speech_uses_system_managed_assets_without_manual_configuration() {
        let profile = apple_speech_profile();
        assert_eq!(profile.kind, "builtin-macos-speech");
        assert_eq!(profile.display_name, "Apple 系统本地识别");
        assert_eq!(profile.auth_kind, "none");
        assert!(profile.config_fields.is_empty());
        assert!(profile.actions.is_empty());
    }

    #[test]
    fn current_settings_preserve_non_secret_config_and_defaults() {
        let settings: ProviderSettings = serde_json::from_str(LEGACY_STATE_JSON).unwrap();
        let mut settings = settings;
        settings.profiles[0].id = BAILIAN_PROVIDER_ID.into();
        settings.profiles[0].kind = "sdk:bailian".into();
        settings.defaults.asr = BAILIAN_PROVIDER_ID.into();

        let normalized = normalize_settings(settings);
        let profile = find_profile(&normalized, BAILIAN_PROVIDER_ID).unwrap();
        assert_eq!(profile.config["apiKey"], "sk-legacy-key");
        assert_eq!(profile.config["hotwords"][0]["text"], "说吧");
        assert_eq!(
            profile.config["vocabularyIds"]["fun-asr-realtime"],
            "vocab-123"
        );
        assert_eq!(normalized.defaults.asr, BAILIAN_PROVIDER_ID);
        // 旧版本没有通用 LLM 配置；升级后统一迁移到内置 Groq 默认项。
        assert_eq!(normalized.defaults.llm, GROQ_LLM_PROVIDER_ID);
        // 旧 JSON 没有 ocr 默认值：normalize 后自动落到内置系统 OCR。
        assert_eq!(normalized.defaults.ocr, SYSTEM_OCR_PROVIDER_ID);
    }

    #[test]
    fn malicious_manifest_cannot_downgrade_secret_fields_exposed_to_webview() {
        let profile = ProviderProfile {
            id: "malicious".into(),
            kind: "plugin:malicious".into(),
            display_name: "Malicious".into(),
            auth_kind: "api-key".into(),
            capabilities: vec!["asr".into()],
            enabled: true,
            config: json!({
                "apiKey": "api-secret",
                "clientSecret": "client-secret",
                "token": "token-secret",
                "passwordValue": "password-secret",
                "region": "cn-test"
            }),
            config_fields: vec![
                ProviderConfigField {
                    key: "clientSecret".into(),
                    label: "Client secret".into(),
                    field_type: "text".into(),
                    secret: false,
                },
                ProviderConfigField {
                    key: "passwordValue".into(),
                    label: "Password".into(),
                    field_type: "password".into(),
                    secret: false,
                },
            ],
            actions: vec![],
        };

        assert!(config_fields_for(&profile).iter().all(|field| field.secret));
        assert_eq!(sanitized_config(&profile), json!({"region": "cn-test"}));
    }

    #[test]
    fn ocr_default_falls_back_to_system_ocr_and_can_be_switched() {
        let mut settings = normalize_settings(ProviderSettings::default());
        assert_eq!(
            default_provider_id(&settings, "ocr"),
            SYSTEM_OCR_PROVIDER_ID
        );
        assert!(has_capability(&settings, SYSTEM_OCR_PROVIDER_ID, "ocr"));

        settings.profiles.push(ProviderProfile {
            id: "plugin-ocr".to_string(),
            kind: "plugin:plugin-ocr".to_string(),
            display_name: "插件 OCR".to_string(),
            auth_kind: "api-key".to_string(),
            capabilities: vec!["ocr".to_string()],
            enabled: true,
            config: json!({}),
            config_fields: vec![],
            actions: vec![],
        });
        set_default_provider(&mut settings, "ocr", "plugin-ocr").unwrap();
        assert_eq!(default_provider_id(&settings, "ocr"), "plugin-ocr");

        let err = set_default_provider(&mut settings, "ocr", BAILIAN_PROVIDER_ID).unwrap_err();
        assert!(err.contains("不支持"));
    }

    #[test]
    fn normalize_settings_migrates_legacy_single_model_config() {
        let mut settings = ProviderSettings::default();
        let profile = settings
            .profiles
            .iter_mut()
            .find(|profile| profile.id == GROQ_LLM_PROVIDER_ID)
            .unwrap();
        profile.config = json!({
            "apiKey": "secret",
            "model": "legacy-model"
        });

        let normalized = normalize_settings(settings);
        let profile = find_profile(&normalized, GROQ_LLM_PROVIDER_ID).unwrap();
        let models = llm_models_from_config(&profile.config);
        assert_eq!(models.len(), 1);
        assert_eq!(models[0], LlmModelConfig::manual("legacy-model"));
        assert_eq!(profile.config["apiKey"], "secret");
        assert!(profile.capabilities.iter().any(|value| value == "asr"));
    }

    #[test]
    fn p0_asr_profiles_use_stable_ids_and_expected_credential_fields() {
        let settings = ProviderSettings::default();
        for id in [VOLCENGINE_PROVIDER_ID, SILICONFLOW_PROVIDER_ID] {
            let profile = find_profile(&settings, id).unwrap();
            assert!(profile.capabilities.iter().any(|value| value == "asr"));
            let fields = config_fields_for(profile);
            assert_eq!(fields.len(), 1);
            assert_eq!(fields[0].key, "apiKey");
            assert!(fields[0].secret);
        }
        let volcengine = find_profile(&settings, VOLCENGINE_PROVIDER_ID).unwrap();
        assert_eq!(config_fields_for(volcengine)[0].label, "APP Key");
        let groq = find_profile(&settings, GROQ_LLM_PROVIDER_ID).unwrap();
        assert!(groq.capabilities.iter().any(|value| value == "llm"));
        assert!(groq.capabilities.iter().any(|value| value == "asr"));
        let groq_fields = config_fields_for(groq);
        assert_eq!(groq_fields.len(), 1);
        assert_eq!(groq_fields[0].key, "apiKey");
    }

    #[test]
    fn normalize_settings_keeps_unknown_profiles() {
        let mut settings = ProviderSettings::default();
        settings.profiles.push(ProviderProfile {
            id: "future-llm".to_string(),
            kind: "future-llm-kind".to_string(),
            display_name: "未来供应商".to_string(),
            auth_kind: "api-key".to_string(),
            capabilities: vec!["llm".to_string()],
            enabled: true,
            config: json!({}),
            config_fields: vec![],
            actions: vec![],
        });

        let normalized = normalize_settings(settings);
        assert!(find_profile(&normalized, "future-llm").is_some());
        assert!(find_profile(&normalized, BAILIAN_PROVIDER_ID).is_some());
    }

    #[test]
    fn normalize_settings_removes_stale_builtin_actions() {
        let mut settings = ProviderSettings::default();
        let profile = settings
            .profiles
            .iter_mut()
            .find(|profile| profile.id == apple_speech::PROVIDER_ID)
            .unwrap();
        profile.actions = vec!["prepareAppleSpeech".to_string()];

        let normalized = normalize_settings(settings);
        let profile = find_profile(&normalized, apple_speech::PROVIDER_ID).unwrap();
        assert!(profile.actions.is_empty());
    }

    #[test]
    fn capability_helpers_are_generic_and_not_hardcoded_to_asr() {
        let settings = ProviderSettings::default();
        assert!(has_capability(&settings, BAILIAN_PROVIDER_ID, "asr"));
        // 百炼同时承担 Qwen-MT 翻译，带 llm 能力。
        assert!(has_capability(&settings, BAILIAN_PROVIDER_ID, "llm"));
        assert_eq!(default_provider_id(&settings, "llm"), GROQ_LLM_PROVIDER_ID);

        let mut settings = settings;
        set_default_provider(&mut settings, "llm", BAILIAN_PROVIDER_ID).unwrap();
        assert_eq!(default_provider_id(&settings, "llm"), BAILIAN_PROVIDER_ID);

        let err = set_default_provider(&mut settings, "llm", "unknown-provider").unwrap_err();
        assert!(err.contains("不支持"));
    }

    #[test]
    fn disabling_provider_excludes_it_and_restores_a_valid_default() {
        let mut settings = ProviderSettings::default();
        settings.profiles.push(ProviderProfile {
            id: "plugin-provider".to_string(),
            kind: "plugin:plugin-provider".to_string(),
            display_name: "插件供应商".to_string(),
            auth_kind: "none".to_string(),
            capabilities: vec!["asr".to_string()],
            enabled: false,
            config: json!({}),
            config_fields: vec![],
            actions: vec![],
        });
        settings.defaults.asr = "plugin-provider".to_string();

        let normalized = normalize_settings(settings);
        assert_eq!(normalized.defaults.asr, BAILIAN_PROVIDER_ID);
        assert!(!has_capability(&normalized, "plugin-provider", "asr"));
    }
}
