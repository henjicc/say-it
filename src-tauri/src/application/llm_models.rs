use crate::commands::common::{provider_profile_for_execution, provider_settings_response};
use crate::persistence::save_persisted_state;
use crate::providers::sdk_runtime::online::{BuiltinSdkRuntime, BuiltinSdkScope};
use crate::providers::{
    llm_models_from_config, normalize_settings, set_llm_models, LlmModelAvailability,
    LlmModelConfig, LlmModelSource, ProviderProfile, ProviderSettingsResponse,
};
use crate::state::RuntimeState;
use serde_json::Value;
use std::collections::HashMap;
use crate::cancellation::CancellationFlag;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::State;

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

/// 内置供应商的模型列表由 SDK 获取：地址按 SDK 预设，硅基流动只列对话模型。
fn sdk_discovery_target(
    profile: &ProviderProfile,
) -> Result<(BuiltinSdkScope, String, Option<String>), String> {
    if profile_value(profile, &crate::providers::llm_api_key_field(profile)).is_empty() {
        return Err(crate::providers::missing_llm_api_key_message(profile));
    }
    let (provider_id, base_url) = crate::providers::sdk_llm_endpoint(profile)?;
    let scope = BuiltinSdkScope::llm(profile, &provider_id, base_url.as_deref())?;
    Ok((scope, provider_id, base_url))
}

fn same_connection(left: &ProviderProfile, right: &ProviderProfile) -> bool {
    left.kind == right.kind
        && profile_value(left, &crate::providers::llm_api_key_field(left))
            == profile_value(right, &crate::providers::llm_api_key_field(right))
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

async fn fetch_model_names(
    state: &RuntimeState,
    profile: &ProviderProfile,
) -> Result<Vec<String>, String> {
    if profile.kind.starts_with("plugin:")
        && profile.capabilities.iter().any(|value| value == "llm")
    {
        let model_id = profile_value(profile, "model");
        let (spec, capability, declared) = {
            let plugins = state
                .plugin_registry
                .lock()
                .map_err(|_| "插件注册表锁定失败".to_string())?;
            let spec = plugins
                .runtime_for_provider(&profile.id)?
                .ok_or_else(|| format!("插件供应商 {} 没有 JavaScript runtime", profile.id))?
                .bind_credentials(state.credentials.clone());
            let capability = plugins
                .llm_capability(&profile.id, model_id)
                .cloned()
                .ok_or_else(|| {
                    format!("插件 {} 未注册模型 {model_id} 的 LLM module", profile.id)
                })?;
            (spec, capability, plugins.llm_model_ids(&profile.id))
        };
        if !capability.model_discovery {
            return Ok(declared);
        }
        let request_id = format!("plugin-llm-discovery-{}", uuid::Uuid::new_v4());
        let profile = profile.clone();
        let value =
            crate::providers::plugin_runtime::spawn_js_worker("plugin-llm-discovery", move || {
                let runtime = crate::providers::plugin_runtime::create_plugin_llm_runtime(
                    spec,
                    &profile,
                    &request_id,
                    MODEL_LIST_TIMEOUT,
                    Arc::new(CancellationFlag::new(false)),
                    None,
                )?;
                runtime.discover_llm(&capability.module_id, &request_id, MODEL_LIST_TIMEOUT)
            })?
            .await
            .map_err(|error| format!("插件 LLM 模型发现工作线程失败：{error}"))??;
        let names = value
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|item| item.get("modelId").and_then(Value::as_str))
            .map(str::to_string)
            .collect::<Vec<_>>();
        if let Some(undeclared) = names.iter().find(|name| !declared.contains(name)) {
            return Err(format!(
                "插件模型发现返回未静态注册的模型 {undeclared}；Plugin API v5 要求先声明对应 module"
            ));
        }
        return Ok(names);
    }
    let (scope, provider_id, base_url) = sdk_discovery_target(profile)?;
    let profile = profile.clone();
    let credentials = state.credentials.clone();
    let request_id = format!("llm-discovery-{}", uuid::Uuid::new_v4());
    let value = crate::providers::plugin_runtime::spawn_js_worker("builtin-llm-discovery", move || {
        let runtime = BuiltinSdkRuntime::create(
            &profile,
            credentials,
            scope,
            request_id,
            Arc::new(CancellationFlag::new(false)),
            HashMap::new(),
        )?;
        runtime.discover_llm(&provider_id, base_url.as_deref(), MODEL_LIST_TIMEOUT)
    })?
    .await
    .map_err(|error| format!("模型发现工作线程失败：{error}"))?
    .map_err(|error| format!("获取模型列表失败：{error}"))?;
    Ok(value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item.get("modelId").and_then(Value::as_str))
        .map(str::to_string)
        .collect())
}

#[tauri::command]
pub(crate) async fn refresh_llm_models(
    app: tauri::AppHandle,
    provider_id: String,
    state: State<'_, RuntimeState>,
) -> Result<ProviderSettingsResponse, String> {
    let attempted_at = now_millis();
    let requested_profile = provider_profile_for_execution(&state, &provider_id)?;
    if requested_profile.kind.starts_with("plugin:")
        && requested_profile
            .capabilities
            .iter()
            .any(|value| value == "llm")
    {
        let plugins = state
            .plugin_registry
            .lock()
            .map_err(|_| "插件注册表锁定失败".to_string())?;
        if plugins
            .runtime_for_provider(&requested_profile.id)?
            .is_none()
        {
            return Err("插件 LLM runtime 不可用".into());
        }
    } else {
        sdk_discovery_target(&requested_profile)?;
    }
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

    let names = fetch_model_names(&state, &requested_profile).await?;
    if names.iter().all(|name| name.trim().is_empty()) {
        return Err("供应商没有返回可用模型，已保留原模型列表".to_string());
    }

    let current_execution_profile = provider_profile_for_execution(&state, &provider_id)?;
    if !same_connection(&current_execution_profile, &requested_profile) {
        return Err("供应商配置在获取模型期间发生变化，请重新刷新".to_string());
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
    Ok(provider_settings_response(
        settings,
        Some(&state.credentials),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn custom_profile(api_key: &str, endpoint: &str) -> ProviderProfile {
        ProviderProfile {
            keep_loaded: false,
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
    fn registered_providers_list_models_on_sdk_preset_addresses() {
        let mut profile = custom_profile("secret", "");
        profile.kind = "sdk:bailian".into();
        let (_, provider_id, base_url) = sdk_discovery_target(&profile).unwrap();
        assert_eq!((provider_id.as_str(), base_url), ("bailian", None));

        let (_, provider_id, base_url) =
            sdk_discovery_target(&custom_profile("secret", "https://example.com/v1")).unwrap();
        assert_eq!(
            (provider_id.as_str(), base_url.as_deref()),
            ("custom", Some("https://example.com/v1"))
        );
    }

    #[test]
    fn builtin_volcengine_lists_models_with_the_ark_key() {
        let mut profile = crate::providers::volcengine_profile();
        profile.config = serde_json::json!({"apiKey": "speech-key"});
        assert_eq!(
            sdk_discovery_target(&profile).unwrap_err(),
            "请先为 火山引擎 设置 方舟 API Key"
        );
        profile.config["arkApiKey"] = "ark-key".into();
        assert!(sdk_discovery_target(&profile).is_ok());
    }

    #[test]
    fn model_refresh_requires_api_key() {
        let error = sdk_discovery_target(&custom_profile("", "https://example.com/v1")).unwrap_err();
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
