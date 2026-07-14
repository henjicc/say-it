use crate::commands::common::provider_settings_response;
use crate::persistence::save_persisted_state;
use crate::providers::{
    llm_models_from_config, normalize_llm_endpoint, normalize_settings, set_llm_models,
    LlmModelAvailability, LlmModelConfig, LlmModelSource, ProviderProfile,
    ProviderSettingsResponse,
};
use crate::state::RuntimeState;
use genai::adapter::AdapterKind;
use genai::resolver::{AuthData, Endpoint, ProviderConfig};
use genai::Client;
use serde_json::Value;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::State;
use tokio::time::timeout;

const MODEL_LIST_TIMEOUT: Duration = Duration::from_secs(20);

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn profile_value<'a>(profile: &'a ProviderProfile, key: &str) -> &'a str {
    profile
        .config
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
}

fn adapter_kind(profile: &ProviderProfile) -> Result<AdapterKind, String> {
    match profile.kind.strip_prefix("llm:") {
        Some("groq") => Ok(AdapterKind::Groq),
        Some("openai") => Ok(AdapterKind::OpenAI),
        Some("anthropic") => Ok(AdapterKind::Anthropic),
        Some("gemini") => Ok(AdapterKind::Gemini),
        Some("deepseek") => Ok(AdapterKind::DeepSeek),
        Some("open_router") => Ok(AdapterKind::OpenRouter),
        Some("custom") => Ok(AdapterKind::OpenAI),
        Some(other) => Err(format!("不支持的大语言模型适配器：{other}")),
        None => Err("供应商不是大语言模型配置".to_string()),
    }
}

fn provider_config(profile: &ProviderProfile) -> Result<ProviderConfig, String> {
    let api_key = profile_value(profile, "apiKey");
    if api_key.is_empty() {
        return Err(format!("请先为 {} 设置 API Key", profile.display_name));
    }
    let auth = AuthData::from_single(api_key.to_string());
    if profile.kind == "llm:custom" {
        let endpoint = profile_value(profile, "endpoint");
        if !(endpoint.starts_with("https://") || endpoint.starts_with("http://")) {
            return Err("自定义大语言模型的接口地址无效".to_string());
        }
        return Ok(ProviderConfig::from((
            Endpoint::from_owned(normalize_llm_endpoint(endpoint)),
            auth,
        )));
    }
    Ok(ProviderConfig::from(auth))
}

fn same_connection(left: &ProviderProfile, right: &ProviderProfile) -> bool {
    left.kind == right.kind
        && profile_value(left, "apiKey") == profile_value(right, "apiKey")
        && profile_value(left, "endpoint") == profile_value(right, "endpoint")
}

pub(crate) fn merge_remote_models(
    existing: Vec<LlmModelConfig>,
    names: Vec<String>,
    current_model: &str,
) -> Vec<LlmModelConfig> {
    let mut remote_names = names
        .into_iter()
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .collect::<Vec<_>>();
    remote_names.sort_by_key(|name| name.to_lowercase());
    remote_names.dedup();

    let mut merged = Vec::with_capacity(remote_names.len() + existing.len());
    for name in remote_names {
        let mut model = existing
            .iter()
            .find(|model| model.name == name)
            .cloned()
            .unwrap_or_else(|| LlmModelConfig::remote(name.clone()));
        model.name = name;
        model.source = LlmModelSource::Remote;
        model.availability = LlmModelAvailability::Available;
        merged.push(model);
    }

    for mut model in existing {
        if merged.iter().any(|item| item.name == model.name) {
            continue;
        }
        match model.source {
            LlmModelSource::Manual => merged.push(model),
            LlmModelSource::Remote if model.name == current_model || model.has_custom_options() => {
                model.availability = LlmModelAvailability::Missing;
                merged.push(model);
            }
            LlmModelSource::Remote => {}
        }
    }
    merged.sort_by_key(|model| model.name.to_lowercase());
    merged
}

async fn fetch_model_names(profile: &ProviderProfile) -> Result<Vec<String>, String> {
    let adapter_kind = adapter_kind(profile)?;
    let provider_config = provider_config(profile)?;
    timeout(
        MODEL_LIST_TIMEOUT,
        Client::default().all_model_names(adapter_kind, provider_config),
    )
    .await
    .map_err(|_| "获取模型列表超时（20 秒）".to_string())?
    .map_err(|error| format!("获取模型列表失败：{error}"))
}

#[tauri::command]
pub(crate) async fn refresh_llm_models(
    app: tauri::AppHandle,
    provider_id: String,
    state: State<'_, RuntimeState>,
) -> Result<ProviderSettingsResponse, String> {
    let attempted_at = now_millis();
    let requested_profile = {
        let guard = state
            .providers
            .lock()
            .map_err(|_| "大语言模型配置锁失败".to_string())?;
        let settings = normalize_settings(guard.clone());
        settings
            .profiles
            .iter()
            .find(|profile| profile.id == provider_id)
            .cloned()
            .ok_or_else(|| format!("供应商 {provider_id} 不存在"))?
    };
    adapter_kind(&requested_profile)?;
    provider_config(&requested_profile)?;
    {
        let mut guard = state
            .providers
            .lock()
            .map_err(|_| "大语言模型配置锁失败".to_string())?;
        let mut settings = normalize_settings(guard.clone());
        let profile = settings
            .profiles
            .iter_mut()
            .find(|profile| profile.id == provider_id)
            .ok_or_else(|| format!("供应商 {provider_id} 不存在"))?;
        profile
            .config
            .as_object_mut()
            .ok_or_else(|| "大语言模型配置格式异常".to_string())?
            .insert("modelListAttemptedAt".to_string(), attempted_at.into());
        *guard = settings;
    }
    save_persisted_state(&app, &state)?;

    let names = fetch_model_names(&requested_profile).await?;
    if names.iter().all(|name| name.trim().is_empty()) {
        return Err("供应商没有返回可用模型，已保留原模型列表".to_string());
    }

    let settings = {
        let mut guard = state
            .providers
            .lock()
            .map_err(|_| "大语言模型配置锁失败".to_string())?;
        let mut settings = normalize_settings(guard.clone());
        let profile = settings
            .profiles
            .iter_mut()
            .find(|profile| profile.id == provider_id)
            .ok_or_else(|| format!("供应商 {provider_id} 已被删除"))?;
        if !same_connection(profile, &requested_profile) {
            return Err("供应商配置在获取模型期间发生变化，请重新刷新".to_string());
        }
        let current_model = profile_value(profile, "model").to_string();
        let models = merge_remote_models(
            llm_models_from_config(&profile.config),
            names,
            &current_model,
        );
        let target = profile
            .config
            .as_object_mut()
            .ok_or_else(|| "大语言模型配置格式异常".to_string())?;
        if current_model.is_empty() {
            if let Some(first) = models.first() {
                target.insert("model".to_string(), first.name.clone().into());
            }
        }
        target.insert("modelsFetchedAt".to_string(), now_millis().into());
        set_llm_models(&mut profile.config, &models)?;
        *guard = settings.clone();
        settings
    };
    save_persisted_state(&app, &state)?;
    Ok(provider_settings_response(settings))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn custom_profile(api_key: &str, endpoint: &str) -> ProviderProfile {
        ProviderProfile {
            id: "custom".into(),
            kind: "llm:custom".into(),
            display_name: "Custom".into(),
            auth_kind: "api-key".into(),
            capabilities: vec!["llm".into()],
            enabled: true,
            config: serde_json::json!({
                "apiKey": api_key,
                "endpoint": endpoint,
                "model": "demo"
            }),
            config_fields: vec![],
            actions: vec![],
        }
    }

    #[test]
    fn custom_provider_uses_openai_adapter_and_normalized_endpoint() {
        let profile = custom_profile("secret", "https://example.com/v1");
        assert_eq!(adapter_kind(&profile).unwrap(), AdapterKind::OpenAI);
        let config = provider_config(&profile).unwrap();
        assert_eq!(
            config.endpoint.as_ref().map(Endpoint::base_url),
            Some("https://example.com/v1/")
        );
    }

    #[test]
    fn model_refresh_requires_api_key() {
        let error = provider_config(&custom_profile("", "https://example.com/v1")).unwrap_err();
        assert!(error.contains("API Key"));
    }

    #[test]
    fn merge_remote_models_deduplicates_and_preserves_manual_and_customized_entries() {
        let mut customized = LlmModelConfig::remote("removed-model");
        customized.reasoning_effort = "high".to_string();
        let models = merge_remote_models(
            vec![
                LlmModelConfig::manual("manual-model"),
                LlmModelConfig::remote("old-model"),
                customized,
            ],
            vec![" z-model ".into(), "a-model".into(), "a-model".into()],
            "old-model",
        );

        assert_eq!(
            models
                .iter()
                .map(|model| model.name.as_str())
                .collect::<Vec<_>>(),
            vec![
                "a-model",
                "manual-model",
                "old-model",
                "removed-model",
                "z-model"
            ]
        );
        assert_eq!(
            models
                .iter()
                .find(|model| model.name == "old-model")
                .unwrap()
                .availability,
            LlmModelAvailability::Missing
        );
        assert_eq!(
            models
                .iter()
                .find(|model| model.name == "removed-model")
                .unwrap()
                .availability,
            LlmModelAvailability::Missing
        );
    }

    #[test]
    fn merge_remote_models_removes_unselected_stale_default_entry() {
        let models = merge_remote_models(
            vec![LlmModelConfig::remote("old-model")],
            vec!["new-model".into()],
            "",
        );
        assert_eq!(models, vec![LlmModelConfig::remote("new-model")]);
    }
}
