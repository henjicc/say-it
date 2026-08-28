use crate::prelude::*;
use crate::state::*;
use tauri_plugin_opener::OpenerExt;

const API_KEY_PAGE_URL: &str =
    "https://bailian.console.aliyun.com/cn-beijing?tab=globalset#/efm/api_key";

#[tauri::command]
pub(crate) fn open_api_key_page(app: tauri::AppHandle) -> Result<(), String> {
    open_external_url(&app, API_KEY_PAGE_URL)
}

#[tauri::command]
pub(crate) fn open_external_link(app: tauri::AppHandle, url: String) -> Result<(), String> {
    let url = url.trim();
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("仅支持打开 http 或 https 链接".to_string());
    }
    open_external_url(&app, url)
}

fn open_external_url(app: &tauri::AppHandle, url: &str) -> Result<(), String> {
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|err| format!("打开浏览器失败：{err}"))
}

pub(crate) fn read_provider_settings(state: &RuntimeState) -> Result<ProviderSettings, String> {
    state
        .providers
        .lock()
        .map_err(|_| "Provider settings lock failed".to_string())
        .map(|v| normalize_settings(v.clone()))
}

/// 只为宿主内置执行链构造带密钥的短生命周期 profile。插件 profile 永不回灌 secret，
/// QuickJS 只能通过受 provider + scope 限制的 RuntimeContext 短生命周期读取本地加密凭据。
pub(crate) fn provider_profile_for_execution(
    state: &RuntimeState,
    provider_id: &str,
) -> Result<ProviderProfile, String> {
    let settings = read_provider_settings(state)?;
    let mut profile = find_profile(&settings, provider_id)
        .cloned()
        .ok_or_else(|| format!("供应商 {provider_id} 不存在"))?;
    if profile.kind.starts_with("plugin:") || profile.kind.starts_with("model-pack:") {
        return Ok(profile);
    }
    for field in config_fields_for(&profile)
        .into_iter()
        .filter(|field| field.secret)
    {
        let key = crate::providers::credential_store::key_for_profile(&profile, &field.key)?;
        if let Some(value) = state.credentials.get(&key)? {
            let config = profile
                .config
                .as_object_mut()
                .ok_or_else(|| "供应商配置格式异常".to_string())?;
            config.insert(field.key, value.into());
        }
    }
    Ok(profile)
}

pub(crate) fn resolve_provider_id(
    state: &RuntimeState,
    capability: &str,
    provider_id: Option<String>,
) -> Result<String, String> {
    let settings = read_provider_settings(state)?;
    let selected = provider_id
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| default_provider_id(&settings, capability));
    if !has_capability(&settings, &selected, capability) {
        return Err(format!("供应商 {selected} 不支持 {capability}"));
    }
    Ok(selected)
}

pub(crate) fn provider_settings_response(
    settings: ProviderSettings,
    credentials: Option<&crate::providers::credential_store::CredentialStoreHandle>,
) -> ProviderSettingsResponse {
    let profiles = settings
        .profiles
        .iter()
        .filter(|profile| {
            profile.kind != crate::providers::apple_speech::PROVIDER_KIND
                || crate::providers::apple_speech::runtime_available()
        })
        .map(|profile| {
            let fields = config_fields_for(profile);
            let has_key = fields.iter().filter(|field| field.secret).any(|field| {
                credentials.is_some_and(|credentials| {
                    crate::providers::credential_store::key_for_profile(profile, &field.key)
                        .and_then(|key| credentials.get(&key))
                        .is_ok_and(|value| value.is_some_and(|value| !value.trim().is_empty()))
                })
            });
            let configured = if profile.kind == crate::providers::apple_speech::PROVIDER_KIND {
                crate::providers::apple_speech::status().available
            } else {
                has_key
            };
            ProviderListItem {
                id: profile.id.clone(),
                kind: profile.kind.clone(),
                display_name: profile.display_name.clone(),
                auth_kind: profile.auth_kind.clone(),
                capabilities: profile.capabilities.clone(),
                enabled: profile.enabled,
                is_default_asr: profile.id == settings.defaults.asr,
                effective_capabilities: profile
                    .capabilities
                    .iter()
                    .filter(|capability| default_provider_id(&settings, capability) == profile.id)
                    .cloned()
                    .collect(),
                config_fields: fields,
                actions: actions_for(profile),
                status: Some(ProviderStatus {
                    has_api_key: Some(has_key),
                    configured: Some(configured),
                }),
                config: sanitized_config(profile),
            }
        })
        .collect::<Vec<_>>();
    ProviderSettingsResponse {
        profiles,
        defaults: settings.defaults,
    }
}
