use crate::commands::common::*;
use crate::persistence::save_persisted_state;
use crate::prelude::*;
use crate::state::*;

fn funasr_config_str(config: &Value, key: &str) -> String {
    config
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string()
}

fn funasr_credentials(state: &tauri::State<'_, RuntimeState>) -> Result<(String, String), String> {
    let settings = read_provider_settings(state)?;
    let profile = find_profile(&settings, FUNASR_PROVIDER_ID)
        .ok_or_else(|| "未找到 Fun-ASR 供应商配置".to_string())?;
    Ok((
        funasr_config_str(&profile.config, "apiKey"),
        funasr_config_str(&profile.config, "vocabularyId"),
    ))
}

/// 用 patch 覆盖 alibabacloud profile 的 config 字段并落盘，返回最新的供应商设置。
fn apply_funasr_patch(
    app: &tauri::AppHandle,
    state: &tauri::State<'_, RuntimeState>,
    patch: Value,
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
            .find(|profile| profile.id == FUNASR_PROVIDER_ID)
            .ok_or_else(|| "未找到 Fun-ASR 供应商配置".to_string())?;
        let patch_obj = patch
            .as_object()
            .ok_or_else(|| "patch 必须是 JSON 对象".to_string())?;
        let target = profile
            .config
            .as_object_mut()
            .ok_or_else(|| "供应商配置格式异常".to_string())?;
        for (key, value) in patch_obj {
            target.insert(key.clone(), value.clone());
        }
        *guard = settings.clone();
        settings
    };
    save_persisted_state(app, state)?;
    Ok(provider_settings_response(settings))
}

/// 把热词列表同步到阿里云百炼（已有词表则更新，没有则新建），并把结果保存到本地配置。
#[tauri::command]
pub(crate) async fn funasr_save_hotwords(
    app: tauri::AppHandle,
    hotwords: Vec<HotwordEntry>,
    state: tauri::State<'_, RuntimeState>,
) -> Result<ProviderSettingsResponse, String> {
    if hotwords.is_empty() {
        return Err("请至少添加一个热词".to_string());
    }
    let (api_key, existing_vocabulary_id) = funasr_credentials(&state)?;
    if api_key.is_empty() {
        return Err("请先保存阿里云百炼 API Key".to_string());
    }

    let vocabulary_id = if existing_vocabulary_id.is_empty() {
        funasr_create_vocabulary(&api_key, &hotwords).await?
    } else {
        match funasr_update_vocabulary(&api_key, &existing_vocabulary_id, &hotwords).await {
            Ok(()) => existing_vocabulary_id,
            // 已保存的词表 ID 可能已失效（例如被在阿里云控制台删除），回退为新建。
            Err(_) => funasr_create_vocabulary(&api_key, &hotwords).await?,
        }
    };

    let hotwords_value = serde_json::to_value(&hotwords).map_err(|e| e.to_string())?;
    apply_funasr_patch(
        &app,
        &state,
        json!({
            "vocabularyId": vocabulary_id,
            "hotwords": hotwords_value,
        }),
    )
}

/// 删除阿里云端的热词列表并清空本地保存的热词配置。
#[tauri::command]
pub(crate) async fn funasr_clear_hotwords(
    app: tauri::AppHandle,
    state: tauri::State<'_, RuntimeState>,
) -> Result<ProviderSettingsResponse, String> {
    let (api_key, vocabulary_id) = funasr_credentials(&state)?;
    if !vocabulary_id.is_empty() && !api_key.is_empty() {
        funasr_delete_vocabulary(&api_key, &vocabulary_id).await?;
    }
    apply_funasr_patch(
        &app,
        &state,
        json!({
            "vocabularyId": "",
            "hotwords": [],
        }),
    )
}
