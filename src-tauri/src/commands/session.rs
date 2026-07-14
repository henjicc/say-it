use crate::commands::common::*;
use crate::persistence::save_persisted_state;
use crate::prelude::*;
use crate::state::*;

const LLM_ADAPTERS: &[&str] = &[
    "groq", "openai", "anthropic", "gemini", "deepseek", "open_router", "custom",
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
    if display_name.is_empty() || model.is_empty() {
        return Err("供应商名称和模型不能为空".to_string());
    }
    let endpoint = request.endpoint.trim();
    if adapter == "custom"
        && !(endpoint.starts_with("https://") || endpoint.starts_with("http://"))
    {
        return Err("自定义供应商必须填写有效的 http 或 https 接口地址".to_string());
    }

    let settings = {
        let mut guard = state
            .providers
            .lock()
            .map_err(|_| "Provider settings lock failed".to_string())?;
        let mut settings = normalize_settings(guard.clone());
        let id = format!("llm-{}", Uuid::new_v4().simple());
        settings.profiles.push(ProviderProfile {
            id: id.clone(),
            kind: format!("llm:{adapter}"),
            display_name: display_name.to_string(),
            auth_kind: "api-key".to_string(),
            capabilities: vec!["llm".to_string()],
            enabled: true,
            config: json!({
                "apiKey": request.api_key.trim(),
                "model": model,
                "endpoint": endpoint,
            }),
            config_fields: vec![],
            actions: vec![],
        });
        if settings.defaults.llm.is_empty() {
            settings.defaults.llm = id;
        }
        *guard = settings.clone();
        settings
    };
    save_persisted_state(&app, &state)?;
    Ok(provider_settings_response(settings))
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
        settings.profiles.retain(|profile| profile.id != provider_id);
        settings = normalize_settings(settings);
        *guard = settings.clone();
        settings
    };
    save_persisted_state(&app, &state)?;
    Ok(provider_settings_response(settings))
}
