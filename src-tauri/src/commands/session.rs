use crate::commands::common::*;
use crate::persistence::save_persisted_state;
use crate::prelude::*;
use crate::state::*;

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
    let mut response = provider_settings_response(settings);
    let registry = state
        .plugin_registry
        .lock()
        .map_err(|_| "插件注册表锁失败".to_string())?;
    for provider in &mut response.profiles {
        if !provider.kind.starts_with("plugin:") || registry.browser_for_provider(&provider.id).is_none() {
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
    Ok(provider_settings_response(settings))
}

#[tauri::command]
pub(crate) fn get_provider_api_key(
    provider_id: String,
    state: tauri::State<'_, RuntimeState>,
) -> Result<String, String> {
    let settings = read_provider_settings(&state)?;
    let profile = find_profile(&settings, &provider_id)
        .ok_or_else(|| format!("供应商 {provider_id} 不存在"))?;
    Ok(profile
        .config
        .get("apiKey")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string())
}

#[tauri::command]
pub(crate) fn update_provider_config(
    app: tauri::AppHandle,
    provider_id: String,
    config: Value,
    state: tauri::State<'_, RuntimeState>,
) -> Result<ProviderSettingsResponse, String> {
    let settings = {
        let mut guard = state
            .providers
            .lock()
            .map_err(|_| "Provider settings lock failed".to_string())?;
        let mut settings = normalize_settings(guard.clone());
        let profile = settings
            .profiles
            .iter_mut()
            .find(|profile| profile.id == provider_id)
            .ok_or_else(|| format!("供应商 {provider_id} 不存在"))?;
        let patch = config
            .as_object()
            .ok_or_else(|| "config 必须是 JSON 对象".to_string())?;
        let target = profile
            .config
            .as_object_mut()
            .ok_or_else(|| "供应商配置格式异常".to_string())?;
        for (key, value) in patch {
            target.insert(key.clone(), value.clone());
        }
        *guard = settings.clone();
        settings
    };
    save_persisted_state(&app, &state)?;
    Ok(provider_settings_response(settings))
}
