use crate::commands::common::*;
use crate::persistence::save_persisted_state;
use crate::prelude::*;
use crate::providers::capabilities::{customization_for, CustomizationProvider};
use crate::state::*;

fn funasr_config_vocabulary_ids(config: &Value) -> HashMap<String, String> {
    config
        .get("vocabularyIds")
        .and_then(|value| serde_json::from_value::<HashMap<String, String>>(value.clone()).ok())
        .unwrap_or_default()
}

fn customization_context_for(
    state: &tauri::State<'_, RuntimeState>,
    provider_id: &str,
) -> Result<(String, CustomizationProvider, HashMap<String, String>), String> {
    let settings = read_provider_settings(state)?;
    let profile = find_profile(&settings, provider_id)
        .ok_or_else(|| format!("供应商 {provider_id} 不存在"))?;
    let provider = customization_for(profile).map_err(|error| error.to_string())?;
    Ok((
        profile.id.clone(),
        provider,
        funasr_config_vocabulary_ids(&profile.config),
    ))
}

/// 用 patch 覆盖 alibabacloud profile 的 config 字段并落盘，返回最新的供应商设置。
fn apply_provider_patch(
    app: &tauri::AppHandle,
    state: &tauri::State<'_, RuntimeState>,
    provider_id: &str,
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
            .find(|profile| profile.id == provider_id)
            .ok_or_else(|| format!("供应商 {provider_id} 不存在"))?;
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

/// 把热词列表同步到阿里云百炼：对每个支持 vocabulary_id 的模型（见 FUNASR_VOCABULARY_TARGETS）
/// 各自维护一份独立词表（已有则更新，没有则新建），全部结果保存到本地配置。
/// 用户只维护这一份热词文本，具体建几份词表、绑定哪个 target_model 完全是内部实现细节。
#[tauri::command]
pub(crate) async fn provider_save_hotwords(
    app: tauri::AppHandle,
    provider_id: String,
    hotwords: Vec<HotwordEntry>,
    state: tauri::State<'_, RuntimeState>,
) -> Result<ProviderSettingsResponse, String> {
    if hotwords.is_empty() {
        return Err("请至少添加一个热词".to_string());
    }
    let (provider_id, provider, existing_ids) = customization_context_for(&state, &provider_id)?;
    provider.ensure_ready()?;

    let mut vocabulary_ids = HashMap::new();
    let mut failures = Vec::new();
    for (target_model, prefix) in provider.targets() {
        let existing = existing_ids.get(*target_model).cloned().unwrap_or_default();
        let result = if existing.is_empty() {
            provider.create(target_model, prefix, &hotwords).await
        } else {
            match provider.update(&existing, &hotwords).await {
                Ok(()) => Ok(existing),
                // 已保存的词表 ID 可能已失效（例如被在阿里云控制台删除），回退为新建。
                Err(_) => provider.create(target_model, prefix, &hotwords).await,
            }
        };
        match result {
            Ok(id) => {
                vocabulary_ids.insert(target_model.to_string(), id);
            }
            Err(err) => failures.push(format!("{target_model}：{err}")),
        }
    }

    let hotwords_value = serde_json::to_value(&hotwords).map_err(|e| e.to_string())?;
    let vocabulary_ids_value = serde_json::to_value(&vocabulary_ids).map_err(|e| e.to_string())?;
    let response = apply_provider_patch(
        &app,
        &state,
        &provider_id,
        json!({
            "vocabularyIds": vocabulary_ids_value,
            "hotwords": hotwords_value,
        }),
    )?;

    if !failures.is_empty() {
        return Err(format!(
            "部分模型的热词保存失败，已成功的部分不受影响，可重试：{}",
            failures.join("；")
        ));
    }
    Ok(response)
}

/// 从阿里云百炼账号下按各模型对应的前缀分别拉取词表（各取修改时间最新一份），覆盖本地保存的热词配置。
#[tauri::command]
pub(crate) async fn provider_sync_hotwords(
    app: tauri::AppHandle,
    provider_id: String,
    state: tauri::State<'_, RuntimeState>,
) -> Result<ProviderSettingsResponse, String> {
    let (provider_id, provider, _) = customization_context_for(&state, &provider_id)?;
    provider.ensure_ready()?;

    let mut vocabulary_ids = HashMap::new();
    let mut hotwords: Option<Vec<HotwordEntry>> = None;
    let mut query_err: Option<String> = None;
    let mut found_any_id = false;
    for (target_model, prefix) in provider.targets() {
        let Ok(ids) = provider.list(prefix).await else {
            continue;
        };
        let Some(vocabulary_id) = ids.into_iter().next() else {
            continue;
        };
        found_any_id = true;
        if hotwords.is_none() {
            match provider.query(&vocabulary_id).await {
                Ok(content) => hotwords = Some(content),
                Err(err) => query_err = Some(err),
            }
        }
        vocabulary_ids.insert(target_model.to_string(), vocabulary_id);
    }

    let hotwords = match hotwords {
        Some(content) => content,
        None if found_any_id => {
            return Err(query_err.unwrap_or_else(|| "查询热词列表内容失败".to_string()));
        }
        None => return Err("云端未找到该账号下的热词列表".to_string()),
    };

    let hotwords_value = serde_json::to_value(&hotwords).map_err(|e| e.to_string())?;
    let vocabulary_ids_value = serde_json::to_value(&vocabulary_ids).map_err(|e| e.to_string())?;
    apply_provider_patch(
        &app,
        &state,
        &provider_id,
        json!({
            "vocabularyIds": vocabulary_ids_value,
            "hotwords": hotwords_value,
        }),
    )
}

/// 删除阿里云端所有模型对应的热词列表并清空本地保存的热词配置；任一模型删除失败则整体返回
/// 错误、不清本地状态，避免出现本地记录丢失但云端词表仍存在的孤儿数据。
#[tauri::command]
pub(crate) async fn provider_clear_hotwords(
    app: tauri::AppHandle,
    provider_id: String,
    state: tauri::State<'_, RuntimeState>,
) -> Result<ProviderSettingsResponse, String> {
    let (provider_id, provider, vocabulary_ids) = customization_context_for(&state, &provider_id)?;
    if !vocabulary_ids.is_empty() {
        for vocabulary_id in vocabulary_ids.values() {
            provider.delete(vocabulary_id).await?;
        }
    }
    apply_provider_patch(
        &app,
        &state,
        &provider_id,
        json!({
            "vocabularyIds": {},
            "hotwords": [],
        }),
    )
}
