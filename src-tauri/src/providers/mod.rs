use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub mod alibabacloud;

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
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderDefaults {
    pub asr: String,
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
    pub status: Option<ProviderStatus>,
    /// 非密钥配置（如热词、语种提示等），用于前端回显；apiKey 等密钥字段会被剔除。
    pub config: Value,
}

pub fn sanitized_config(config: &Value) -> Value {
    let mut sanitized = config.clone();
    if let Some(obj) = sanitized.as_object_mut() {
        obj.remove("apiKey");
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
        display_name: "Fun-ASR（阿里云百炼）".to_string(),
        auth_kind: "api-key".to_string(),
        capabilities: vec!["asr".to_string()],
        enabled: true,
        config: json!({
            "apiKey": "",
            "vocabularyId": "",
            "hotwords": [],
            "languageHints": [],
            "semanticPunctuationEnabled": false,
            "maxSentenceSilence": 1300,
            "multiThresholdModeEnabled": false,
            "heartbeat": false,
            "speechNoiseThreshold": null
        }),
    }
}

pub fn find_profile<'a>(settings: &'a ProviderSettings, id: &str) -> Option<&'a ProviderProfile> {
    settings.profiles.iter().find(|profile| profile.id == id)
}

pub fn normalize_settings(mut settings: ProviderSettings) -> ProviderSettings {
    if !settings
        .profiles
        .iter()
        .any(|profile| profile.id == FUNASR_PROVIDER_ID)
    {
        settings.profiles.insert(0, funasr_profile());
    }

    settings.profiles.retain(|profile| profile.id == FUNASR_PROVIDER_ID);
    for profile in &mut settings.profiles {
        if profile.display_name.trim().is_empty() {
            profile.display_name = "Fun-ASR（阿里云百炼）".to_string();
        }
        profile.enabled = true;
        profile.capabilities = vec!["asr".to_string()];
    }

    if !has_capability(&settings, &settings.defaults.asr, "asr") {
        settings.defaults.asr = FUNASR_PROVIDER_ID.to_string();
    }

    settings
}

pub fn has_capability(settings: &ProviderSettings, provider_id: &str, capability: &str) -> bool {
    capability == "asr"
        && settings.profiles.iter().any(|profile| {
            profile.enabled
                && profile.id == provider_id
                && profile.capabilities.iter().any(|item| item == capability)
        })
}

pub fn default_provider_id(settings: &ProviderSettings, capability: &str) -> String {
    if capability == "asr" {
        settings.defaults.asr.clone()
    } else {
        FUNASR_PROVIDER_ID.to_string()
    }
}

pub fn set_default_provider(
    settings: &mut ProviderSettings,
    capability: &str,
    provider_id: &str,
) -> Result<(), String> {
    if capability != "asr" {
        return Err(format!("不支持的能力类型：{capability}"));
    }
    if !has_capability(settings, provider_id, capability) {
        return Err(format!("供应商 {provider_id} 不支持 {capability}"));
    }
    settings.defaults.asr = provider_id.to_string();
    Ok(())
}
