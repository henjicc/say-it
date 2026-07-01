use crate::prelude::*;
use crate::state::*;

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
