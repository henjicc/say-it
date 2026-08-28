use crate::commands::common::*;
use crate::persistence::save_persisted_state;
use crate::prelude::*;
use crate::providers::credential_store::{key_for_profile, redact_error, CredentialKey};
use crate::state::*;

const LLM_ADAPTERS: &[&str] = &[
    "groq",
    "openai",
    "anthropic",
    "gemini",
    "volcengine",
    "kimi",
    "bigmodel",
    "deepseek",
    "mimo",
    "bailian",
    "minimax",
    "open_router",
    "custom",
];

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AddLlmProviderRequest {
    adapter: String,
    display_name: String,
    model: String,
    #[serde(default)]
    api_key: String,
    #[serde(default)]
    endpoint: String,
}

type CredentialJournal = Vec<(CredentialKey, Option<String>)>;

fn rollback_credential_changes(
    state: &RuntimeState,
    journal: &CredentialJournal,
) -> Result<(), String> {
    let secrets = journal
        .iter()
        .filter_map(|(_, previous)| previous.clone())
        .collect::<Vec<_>>();
    let mut errors = Vec::new();
    for (key, previous) in journal.iter().rev() {
        let result = match previous {
            Some(value) => state.credentials.store().set(key, value),
            None => state.credentials.store().delete(key),
        };
        if let Err(error) = result {
            errors.push(error);
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(redact_error(
            &format!("恢复本地加密凭据失败：{}", errors.join("；")),
            &secrets,
        ))
    }
}

fn write_credential_changes(
    state: &RuntimeState,
    changes: &[(CredentialKey, String)],
) -> Result<CredentialJournal, String> {
    let secrets = changes
        .iter()
        .map(|(_, value)| value.clone())
        .collect::<Vec<_>>();
    let mut journal = Vec::new();
    for (key, value) in changes {
        let previous = match state.credentials.get(key) {
            Ok(previous) => previous,
            Err(error) => {
                let _ = rollback_credential_changes(state, &journal);
                return Err(redact_error(&error, &secrets));
            }
        };
        if previous.as_deref() == Some(value.as_str()) {
            continue;
        }
        if let Err(error) = state.credentials.write_verified(key, value) {
            let rollback = rollback_credential_changes(state, &journal);
            let error = redact_error(&error, &secrets);
            return match rollback {
                Ok(()) => Err(error),
                Err(rollback) => Err(format!("{error}；{}", redact_error(&rollback, &secrets))),
            };
        }
        journal.push((key.clone(), previous));
    }
    Ok(journal)
}

#[tauri::command]
pub(crate) fn get_session_status(
    state: tauri::State<'_, RuntimeState>,
) -> Result<SessionStatus, String> {
    let providers = read_provider_settings(&state)?;
    Ok(SessionStatus {
        default_asr_provider: providers.defaults.asr,
    })
}

#[tauri::command]
pub(crate) fn list_providers(
    state: tauri::State<'_, RuntimeState>,
) -> Result<ProviderSettingsResponse, String> {
    let settings = read_provider_settings(&state)?;
    let mut response = provider_settings_response(settings, Some(&state.credentials));
    let registry = state
        .plugin_registry
        .lock()
        .map_err(|_| "插件注册表锁失败".to_string())?;
    for provider in &mut response.profiles {
        if !provider.kind.starts_with("plugin:")
            || registry.browser_for_provider(&provider.id).is_none()
        {
            continue;
        }
        let configured = registry
            .runtime_for_provider(&provider.id)?
            .map(|spec| {
                crate::providers::plugin_secrets::load_session(&spec)
                    .map(|session| !session.is_null())
                    .unwrap_or(false)
            })
            .unwrap_or(false);
        if let Some(status) = &mut provider.status {
            status.configured = Some(configured);
        }
    }
    Ok(response)
}

#[tauri::command]
pub(crate) fn set_default_provider(
    app: tauri::AppHandle,
    request: SetDefaultProviderRequest,
    state: tauri::State<'_, RuntimeState>,
) -> Result<ProviderSettingsResponse, String> {
    let settings = {
        let mut guard = state
            .providers
            .lock()
            .map_err(|_| "Provider settings lock failed".to_string())?;
        let mut settings = normalize_settings(guard.clone());
        set_default_provider_value(&mut settings, &request.capability, &request.provider_id)?;
        *guard = settings.clone();
        settings
    };
    save_persisted_state(&app, &state)?;
    Ok(provider_settings_response(
        settings,
        Some(&state.credentials),
    ))
}

#[tauri::command]
pub(crate) fn update_provider_config(
    app: tauri::AppHandle,
    provider_id: String,
    config: Value,
    state: tauri::State<'_, RuntimeState>,
) -> Result<ProviderSettingsResponse, String> {
    let (previous_settings, settings, credential_changes) = {
        let guard = state
            .providers
            .lock()
            .map_err(|_| "Provider settings lock failed".to_string())?;
        let previous_settings = guard.clone();
        let mut settings = normalize_settings(previous_settings.clone());
        let profile = settings
            .profiles
            .iter_mut()
            .find(|profile| profile.id == provider_id)
            .ok_or_else(|| format!("供应商 {provider_id} 不存在"))?;
        let patch = config
            .as_object()
            .ok_or_else(|| "config 必须是 JSON 对象".to_string())?;
        let secret_fields = crate::providers::secret_config_keys(profile);
        let credential_profile = profile.clone();
        let mut credential_changes = Vec::new();
        let target = profile
            .config
            .as_object_mut()
            .ok_or_else(|| "供应商配置格式异常".to_string())?;
        for (key, value) in patch {
            if secret_fields.contains(key) || crate::providers::is_sensitive_config_key(key) {
                let value = value
                    .as_str()
                    .ok_or_else(|| format!("密钥字段 {key} 必须是字符串"))?
                    .trim();
                if !value.is_empty() {
                    credential_changes.push((
                        key_for_profile(&credential_profile, key)?,
                        value.to_string(),
                    ));
                }
                target.remove(key);
            } else {
                target.insert(key.clone(), value.clone());
            }
        }
        (previous_settings, settings, credential_changes)
    };
    let journal = write_credential_changes(&state, &credential_changes)?;
    match state.providers.lock() {
        Ok(mut guard) => *guard = settings.clone(),
        Err(_) => {
            let rollback = rollback_credential_changes(&state, &journal);
            return match rollback {
                Ok(()) => Err("Provider settings lock failed".into()),
                Err(rollback) => Err(format!("Provider settings lock failed；{rollback}")),
            };
        }
    }
    if let Err(error) = save_persisted_state(&app, &state) {
        if let Ok(mut guard) = state.providers.lock() {
            *guard = previous_settings;
        }
        let rollback = rollback_credential_changes(&state, &journal);
        return match rollback {
            Ok(()) => Err(error),
            Err(rollback) => Err(format!("{error}；{rollback}")),
        };
    }
    Ok(provider_settings_response(
        settings,
        Some(&state.credentials),
    ))
}

#[tauri::command]
pub(crate) fn add_llm_provider(
    app: tauri::AppHandle,
    request: AddLlmProviderRequest,
    state: tauri::State<'_, RuntimeState>,
) -> Result<ProviderSettingsResponse, String> {
    let adapter = request.adapter.trim();
    if !LLM_ADAPTERS.contains(&adapter) {
        return Err(format!("不支持的大语言模型适配器：{adapter}"));
    }
    let display_name = request.display_name.trim();
    let model = request.model.trim();
    let api_key = request.api_key.trim();
    if display_name.is_empty() {
        return Err("供应商名称不能为空".to_string());
    }
    if api_key.is_empty() {
        return Err("API Key 不能为空".to_string());
    }
    let endpoint = request.endpoint.trim();
    if adapter == "custom" && !(endpoint.starts_with("https://") || endpoint.starts_with("http://"))
    {
        return Err("自定义供应商必须填写有效的 http 或 https 接口地址".to_string());
    }

    let (previous_settings, settings, credential_change) = {
        let guard = state
            .providers
            .lock()
            .map_err(|_| "Provider settings lock failed".to_string())?;
        let previous_settings = guard.clone();
        let mut settings = normalize_settings(previous_settings.clone());
        let id = format!("llm-{}", Uuid::new_v4().simple());
        let models = if model.is_empty() {
            Vec::new()
        } else {
            vec![crate::providers::LlmModelConfig::manual(model)]
        };
        settings.profiles.push(ProviderProfile {
            id: id.clone(),
            kind: format!("llm:{adapter}"),
            display_name: display_name.to_string(),
            auth_kind: "api-key".to_string(),
            capabilities: vec!["llm".to_string()],
            enabled: true,
            config: json!({
                "model": model,
                "endpoint": endpoint,
                "models": models,
            }),
            config_fields: vec![],
            actions: vec![],
        });
        if settings.defaults.llm.is_empty() {
            settings.defaults.llm = id.clone();
        }
        let key = CredentialKey::provider(&id, "apiKey")?;
        (previous_settings, settings, (key, api_key.to_string()))
    };
    let journal = write_credential_changes(&state, &[credential_change])?;
    match state.providers.lock() {
        Ok(mut guard) => *guard = settings.clone(),
        Err(_) => {
            let rollback = rollback_credential_changes(&state, &journal);
            return match rollback {
                Ok(()) => Err("Provider settings lock failed".into()),
                Err(rollback) => Err(format!("Provider settings lock failed；{rollback}")),
            };
        }
    }
    if let Err(error) = save_persisted_state(&app, &state) {
        if let Ok(mut guard) = state.providers.lock() {
            *guard = previous_settings;
        }
        let rollback = rollback_credential_changes(&state, &journal);
        return match rollback {
            Ok(()) => Err(error),
            Err(rollback) => Err(format!("{error}；{rollback}")),
        };
    }
    Ok(provider_settings_response(
        settings,
        Some(&state.credentials),
    ))
}

#[tauri::command]
pub(crate) fn remove_llm_provider(
    app: tauri::AppHandle,
    provider_id: String,
    state: tauri::State<'_, RuntimeState>,
) -> Result<ProviderSettingsResponse, String> {
    if provider_id == GROQ_LLM_PROVIDER_ID {
        return Err("内置 Groq 配置不能删除".to_string());
    }
    let settings = {
        let mut guard = state
            .providers
            .lock()
            .map_err(|_| "Provider settings lock failed".to_string())?;
        let mut settings = normalize_settings(guard.clone());
        let profile = find_profile(&settings, &provider_id)
            .ok_or_else(|| format!("供应商 {provider_id} 不存在"))?;
        if !profile.kind.starts_with("llm:") {
            return Err("只能删除大语言模型供应商".to_string());
        }
        crate::providers::remove_profile_preserving_credentials(&mut settings, &provider_id);
        settings = normalize_settings(settings);
        *guard = settings.clone();
        settings
    };
    save_persisted_state(&app, &state)?;
    Ok(provider_settings_response(
        settings,
        Some(&state.credentials),
    ))
}
