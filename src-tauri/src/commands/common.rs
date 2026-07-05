use crate::prelude::*;
use crate::state::*;

const API_KEY_PAGE_URL: &str =
    "https://bailian.console.aliyun.com/cn-beijing?tab=globalset#/efm/api_key";

#[tauri::command]
pub(crate) fn open_api_key_page() -> Result<(), String> {
    open_external_url(API_KEY_PAGE_URL)
}

#[tauri::command]
pub(crate) fn open_external_link(url: String) -> Result<(), String> {
    let url = url.trim();
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("仅支持打开 http 或 https 链接".to_string());
    }
    open_external_url(url)
}

fn open_external_url(url: &str) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    let status = std::process::Command::new("cmd")
        .args(["/C", "start", "", url])
        .status();

    #[cfg(target_os = "macos")]
    let status = std::process::Command::new("open").arg(url).status();

    #[cfg(all(unix, not(target_os = "macos")))]
    let status = std::process::Command::new("xdg-open").arg(url).status();

    status
        .map_err(|err| format!("打开浏览器失败：{err}"))
        .and_then(|status| {
            if status.success() {
                Ok(())
            } else {
                Err(format!("打开浏览器失败，退出码：{status}"))
            }
        })
}

pub(crate) fn read_provider_settings(
    state: &tauri::State<'_, RuntimeState>,
) -> Result<ProviderSettings, String> {
    state
        .providers
        .lock()
        .map_err(|_| "Provider settings lock failed".to_string())
        .map(|v| normalize_settings(v.clone()))
}

pub(crate) fn resolve_provider_id(
    state: &tauri::State<'_, RuntimeState>,
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

pub(crate) fn provider_settings_response(settings: ProviderSettings) -> ProviderSettingsResponse {
    let profiles = settings
        .profiles
        .iter()
        .map(|profile| {
            let has_key = profile
                .config
                .get("apiKey")
                .and_then(Value::as_str)
                .map(|v| !v.trim().is_empty())
                .unwrap_or(false);
            ProviderListItem {
                id: profile.id.clone(),
                kind: profile.kind.clone(),
                display_name: profile.display_name.clone(),
                auth_kind: profile.auth_kind.clone(),
                capabilities: profile.capabilities.clone(),
                enabled: profile.enabled,
                is_default_asr: profile.id == settings.defaults.asr,
                status: Some(ProviderStatus {
                    has_api_key: Some(has_key),
                }),
                config: sanitized_config(&profile.config),
            }
        })
        .collect::<Vec<_>>();
    ProviderSettingsResponse {
        profiles,
        defaults: settings.defaults,
    }
}
