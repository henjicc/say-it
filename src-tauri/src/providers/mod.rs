use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use connector::RealtimeAsrConnector;

pub mod alibabacloud;
pub mod browser_session_capture;
pub mod capabilities;
pub mod connector;
pub mod local_asr;
pub mod model_download;
pub mod plugin;
pub mod plugin_package;
pub mod plugin_runtime;
pub mod plugin_secrets;
pub mod registry;
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

pub const FUNASR_PROVIDER_ID: &str = "funasr";
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
            profiles: vec![funasr_profile(), groq_llm_profile(), windows_ocr_profile()],
            defaults: ProviderDefaults {
                asr: FUNASR_PROVIDER_ID.to_string(),
                llm: GROQ_LLM_PROVIDER_ID.to_string(),
                translation: FUNASR_PROVIDER_ID.to_string(),
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

pub fn config_fields_for(profile: &ProviderProfile) -> Vec<ProviderConfigField> {
    if !profile.config_fields.is_empty() {
        return profile.config_fields.clone();
    }
    match profile.kind.as_str() {
        "alibabacloud-funasr" => vec![ProviderConfigField {
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
        _ => Vec::new(),
    }
}

pub fn actions_for(profile: &ProviderProfile) -> Vec<String> {
    if !profile.actions.is_empty() {
        return profile.actions.clone();
    }
    match profile.kind.as_str() {
        "alibabacloud-funasr" => vec!["manageHotwords".into(), "testRealtimeAsr".into()],
        _ => Vec::new(),
    }
}

pub fn sanitized_config(profile: &ProviderProfile) -> Value {
    let mut sanitized = profile.config.clone();
    if let Some(obj) = sanitized.as_object_mut() {
        obj.remove("apiKey");
        for field in config_fields_for(profile) {
            if field.secret {
                obj.remove(&field.key);
            }
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

pub fn funasr_profile() -> ProviderProfile {
    ProviderProfile {
        id: FUNASR_PROVIDER_ID.to_string(),
        kind: "alibabacloud-funasr".to_string(),
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
            "apiKey": "",
            "vocabularyIds": {},
            "hotwords": [],
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

pub fn groq_llm_profile() -> ProviderProfile {
    ProviderProfile {
        id: GROQ_LLM_PROVIDER_ID.to_string(),
        kind: "llm:groq".to_string(),
        display_name: "Groq".to_string(),
        auth_kind: "api-key".to_string(),
        capabilities: vec!["llm".to_string()],
        enabled: true,
        config: json!({
            "apiKey": "",
            "model": "openai/gpt-oss-20b",
            "models": [LlmModelConfig::manual("openai/gpt-oss-20b")]
        }),
        config_fields: vec![],
        actions: vec![],
    }
}

/// 内置 Windows 系统 OCR：无配置项，识别调用见 `capabilities::OcrProvider::System`。
pub fn windows_ocr_profile() -> ProviderProfile {
    ProviderProfile {
        id: SYSTEM_OCR_PROVIDER_ID.to_string(),
        kind: "builtin-windows-ocr".to_string(),
        display_name: "Windows 系统 OCR".to_string(),
        auth_kind: "none".to_string(),
        capabilities: vec!["ocr".to_string()],
        enabled: true,
        config: json!({}),
        config_fields: vec![],
        actions: vec![],
    }
}

pub fn find_profile<'a>(settings: &'a ProviderSettings, id: &str) -> Option<&'a ProviderProfile> {
    settings.profiles.iter().find(|profile| profile.id == id)
}

/// 内置供应商清单：新增供应商时在这里追加一个 profile 构造函数。
pub fn builtin_profiles() -> Vec<ProviderProfile> {
    vec![funasr_profile(), groq_llm_profile(), windows_ocr_profile()]
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

fn normalize_llm_profile_config(profile: &mut ProviderProfile) {
    if !profile.kind.starts_with("llm:") {
        return;
    }
    if !profile.config.is_object() {
        profile.config = json!({});
    }
    let models = llm_models_from_config(&profile.config);
    let _ = set_llm_models(&mut profile.config, &models);
}

pub fn normalize_settings(mut settings: ProviderSettings) -> ProviderSettings {
    let migrate_legacy_llm_default = settings.defaults.llm.is_empty()
        || settings.defaults.llm == FUNASR_PROVIDER_ID;
    for builtin in builtin_profiles() {
        match settings.profiles.iter_mut().find(|p| p.id == builtin.id) {
            Some(existing) => {
                // 只修正内置供应商的固定字段，config 保留用户已保存的值（apiKey/热词等）。
                existing.kind = builtin.kind;
                existing.display_name = builtin.display_name;
                existing.auth_kind = builtin.auth_kind;
                existing.capabilities = builtin.capabilities;
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

/// 按供应商 `kind` 选取实时识别连接器，返回连接器与实际生效的模型名。
/// 新增供应商时只需在这里加一个 match 分支。
pub fn realtime_connector_for(
    kind: &str,
    config: &Value,
    model_override: Option<&str>,
) -> Result<(Box<dyn RealtimeAsrConnector>, String), String> {
    match kind {
        "alibabacloud-funasr" => {
            let params: alibabacloud::FunAsrParams =
                serde_json::from_value(config.clone()).map_err(|e| e.to_string())?;
            if params.api_key.trim().is_empty() {
                return Err("请先在设置中填写阿里云百炼 API Key".to_string());
            }
            let model = params.realtime_model(model_override);
            Ok((alibabacloud::realtime_connector(&params, &model), model))
        }
        other => Err(format!("当前版本不支持供应商类型：{other}")),
    }
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
    fn legacy_json_without_llm_field_preserves_key_hotwords_and_defaults() {
        let settings: ProviderSettings = serde_json::from_str(LEGACY_STATE_JSON).unwrap();
        assert_eq!(settings.defaults.llm, "");

        let normalized = normalize_settings(settings);
        let profile = find_profile(&normalized, FUNASR_PROVIDER_ID).unwrap();
        assert_eq!(profile.config["apiKey"], "sk-legacy-key");
        assert_eq!(profile.config["hotwords"][0]["text"], "说吧");
        assert_eq!(
            profile.config["vocabularyIds"]["fun-asr-realtime"],
            "vocab-123"
        );
        assert_eq!(normalized.defaults.asr, "funasr");
        // 旧版本没有通用 LLM 配置；升级后统一迁移到内置 Groq 默认项。
        assert_eq!(normalized.defaults.llm, GROQ_LLM_PROVIDER_ID);
        // 旧 JSON 没有 ocr 默认值：normalize 后自动落到内置系统 OCR。
        assert_eq!(normalized.defaults.ocr, SYSTEM_OCR_PROVIDER_ID);
    }

    #[test]
    fn ocr_default_falls_back_to_system_ocr_and_can_be_switched() {
        let mut settings = normalize_settings(ProviderSettings::default());
        assert_eq!(default_provider_id(&settings, "ocr"), SYSTEM_OCR_PROVIDER_ID);
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

        let err = set_default_provider(&mut settings, "ocr", FUNASR_PROVIDER_ID).unwrap_err();
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
        assert!(find_profile(&normalized, FUNASR_PROVIDER_ID).is_some());
    }

    #[test]
    fn capability_helpers_are_generic_and_not_hardcoded_to_asr() {
        let settings = ProviderSettings::default();
        assert!(has_capability(&settings, FUNASR_PROVIDER_ID, "asr"));
        // funasr（阿里云百炼）同时承担 Qwen-MT 翻译，带 llm 能力。
        assert!(has_capability(&settings, FUNASR_PROVIDER_ID, "llm"));
        assert_eq!(default_provider_id(&settings, "llm"), GROQ_LLM_PROVIDER_ID);

        let mut settings = settings;
        set_default_provider(&mut settings, "llm", FUNASR_PROVIDER_ID).unwrap();
        assert_eq!(default_provider_id(&settings, "llm"), FUNASR_PROVIDER_ID);

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
        assert_eq!(normalized.defaults.asr, FUNASR_PROVIDER_ID);
        assert!(!has_capability(&normalized, "plugin-provider", "asr"));
    }

    #[test]
    fn realtime_connector_for_rejects_unknown_kind() {
        match realtime_connector_for("unknown-kind", &json!({}), None) {
            Err(err) => assert!(err.contains("不支持供应商类型")),
            Ok(_) => panic!("expected an error for unknown kind"),
        }
    }
}
