use crate::persistence::save_persisted_state;
use crate::providers::plugin::{load_registry, PluginRegistrySnapshot};
use crate::state::RuntimeState;
use tauri::Manager;

pub fn initialize(app: &tauri::AppHandle) -> Result<(), String> {
    let registry = load_registry(app)?;
    let state = app.state::<RuntimeState>();
    {
        let mut providers = state
            .providers
            .lock()
            .map_err(|_| "供应商配置锁失败".to_string())?;
        registry.merge_provider_profiles(&mut providers);
    }
    *state
        .plugin_registry
        .lock()
        .map_err(|_| "插件注册表锁失败".to_string())? = registry;
    Ok(())
}

#[tauri::command]
pub(crate) fn list_provider_plugins(
    state: tauri::State<'_, RuntimeState>,
) -> Result<PluginRegistrySnapshot, String> {
    state
        .plugin_registry
        .lock()
        .map_err(|_| "插件注册表锁失败".to_string())
        .map(|registry| registry.snapshot())
}

#[tauri::command]
pub(crate) fn reload_provider_plugins(
    app: tauri::AppHandle,
    state: tauri::State<'_, RuntimeState>,
) -> Result<PluginRegistrySnapshot, String> {
    let registry = load_registry(&app)?;
    {
        let mut providers = state
            .providers
            .lock()
            .map_err(|_| "供应商配置锁失败".to_string())?;
        registry.merge_provider_profiles(&mut providers);
    }
    let snapshot = registry.snapshot();
    *state
        .plugin_registry
        .lock()
        .map_err(|_| "插件注册表锁失败".to_string())? = registry;
    save_persisted_state(&app, &state)?;
    Ok(snapshot)
}
