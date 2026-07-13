use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use connector::RealtimeAsrConnector;

pub mod alibabacloud;
pub mod capabilities;
pub mod connector;
pub mod plugin;
pub mod registry;
#[cfg(test)]
mod testing;

pub const FUNASR_PROVIDER_ID: &str = "funasr";

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
            profiles: vec![funasr_profile()],
            defaults: ProviderDefaults {
                asr: FUNASR_PROVIDER_ID.to_string(),
                llm: String::new(),
            },
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderStatus {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub has_api_key: Option<bool>,
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
            key: "apiKey".into(), label: "API Key".into(), field_type: "password".into(), secret: true,
        }],
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
        capabilities: vec!["asr".to_string(), "llm".to_string()],
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

pub fn find_profile<'a>(settings: &'a ProviderSettings, id: &str) -> Option<&'a ProviderProfile> {
    settings.profiles.iter().find(|profile| profile.id == id)
}

/// 内置供应商清单：新增供应商时在这里追加一个 profile 构造函数。
pub fn builtin_profiles() -> Vec<ProviderProfile> {
    vec![funasr_profile()]
}

pub fn normalize_settings(mut settings: ProviderSettings) -> ProviderSettings {
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
    // 未知 id 的 profile（用户手工配置或未来供应商）原样保留，不再删除。

    settings.defaults.asr = valid_or_fallback(&settings, &settings.defaults.asr, "asr");
    settings.defaults.llm = valid_or_fallback(&settings, &settings.defaults.llm, "llm");

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
        assert_eq!(profile.config["vocabularyIds"]["fun-asr-realtime"], "vocab-123");
        assert_eq!(normalized.defaults.asr, "funasr");
        // funasr 现在也带 llm 能力，未设置过 defaults.llm 的旧状态会自动回落到它，
        // 这样 resolve_provider_id(_, "llm", None) 才能直接找到可用供应商。
        assert_eq!(normalized.defaults.llm, "funasr");
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
        assert_eq!(default_provider_id(&settings, "llm"), "");

        let mut settings = settings;
        set_default_provider(&mut settings, "llm", FUNASR_PROVIDER_ID).unwrap();
        assert_eq!(default_provider_id(&settings, "llm"), FUNASR_PROVIDER_ID);

        let err = set_default_provider(&mut settings, "llm", "unknown-provider").unwrap_err();
        assert!(err.contains("不支持"));
    }

    #[test]
    fn realtime_connector_for_rejects_unknown_kind() {
        match realtime_connector_for("unknown-kind", &json!({}), None) {
            Err(err) => assert!(err.contains("不支持供应商类型")),
            Ok(_) => panic!("expected an error for unknown kind"),
        }
    }
}
