//! 把全局热词（`application::customization`）同步到各供应商的云端词表。
//!
//! 热词本身不再按供应商保存：这里只负责「推送 / 拉取 / 清除」这三个厂商侧动作，
//! 以及记录厂商返回的 `vocabularyIds`（词表 ID 是厂商侧资源，必须留在供应商配置里）。
use crate::application::customization::CustomizationPrefs;
use crate::commands::common::*;
use crate::persistence::save_persisted_state;
use crate::prelude::*;
use crate::providers::capabilities::{customization_for_with_plugin, CustomizationProvider};
use crate::state::*;

/// 一个供应商的同步结果。整体不因单个供应商失败而中断，前端按条展示。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProviderSyncResult {
    pub(crate) provider_id: String,
    pub(crate) display_name: String,
    pub(crate) ok: bool,
    pub(crate) message: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CustomizationSyncResponse {
    pub(crate) results: Vec<ProviderSyncResult>,
    pub(crate) providers: ProviderSettingsResponse,
}

fn funasr_config_vocabulary_ids(config: &Value) -> HashMap<String, String> {
    config
        .get("vocabularyIds")
        .and_then(|value| serde_json::from_value::<HashMap<String, String>>(value.clone()).ok())
        .unwrap_or_default()
}

/// 支持热词同步的已启用供应商。判定与设置页一致：声明 `customization` 能力，
/// 或提供 `manageHotwords` 动作。
fn sync_targets(state: &tauri::State<'_, RuntimeState>) -> Result<Vec<(String, String)>, String> {
    let settings = read_provider_settings(state)?;
    Ok(settings
        .profiles
        .iter()
        .filter(|profile| profile.enabled)
        .filter(|profile| {
            profile.capabilities.iter().any(|item| item == "customization")
                || crate::providers::actions_for(profile)
                    .iter()
                    .any(|item| item == "manageHotwords")
        })
        .map(|profile| (profile.id.clone(), profile.display_name.clone()))
        .collect())
}

fn customization_context_for(
    state: &tauri::State<'_, RuntimeState>,
    provider_id: &str,
) -> Result<(String, CustomizationProvider, HashMap<String, String>), String> {
    let settings = read_provider_settings(state)?;
    let profile = find_profile(&settings, provider_id)
        .ok_or_else(|| format!("供应商 {provider_id} 不存在"))?;
    let plugin = state
        .plugin_registry
        .lock()
        .map_err(|_| "插件注册表锁失败".to_string())?
        .runtime_for_provider(provider_id)?;
    let provider =
        customization_for_with_plugin(profile, plugin).map_err(|error| error.to_string())?;
    Ok((
        profile.id.clone(),
        provider,
        funasr_config_vocabulary_ids(&profile.config),
    ))
}

/// 用 patch 覆盖 profile 的 config 字段并落盘，返回最新的供应商设置。
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

/// 把全局热词推送到一个供应商：插件走统一的 `setHotwords`；阿里云为每个需要独立词表的
/// 模型各维护一份（已有则更新，没有则新建），词表 ID 保存回供应商配置。
async fn push_to_provider(
    app: &tauri::AppHandle,
    state: &tauri::State<'_, RuntimeState>,
    provider_id: &str,
    hotwords: &[HotwordEntry],
) -> Result<(), String> {
    let (provider_id, provider, existing_ids) = customization_context_for(state, provider_id)?;
    provider.ensure_ready()?;

    if provider.is_plugin() {
        provider.set_hotwords(hotwords).await?;
        return Ok(());
    }

    let mut vocabulary_ids = HashMap::new();
    let mut failures = Vec::new();
    for (target_model, prefix) in provider.targets() {
        let existing = existing_ids.get(*target_model).cloned().unwrap_or_default();
        let result = if existing.is_empty() {
            provider.create(target_model, prefix, hotwords).await
        } else {
            match provider.update(&existing, hotwords).await {
                Ok(()) => Ok(existing),
                // 已保存的词表 ID 可能已失效（例如被在阿里云控制台删除），回退为新建。
                Err(_) => provider.create(target_model, prefix, hotwords).await,
            }
        };
        match result {
            Ok(id) => {
                vocabulary_ids.insert(target_model.to_string(), id);
            }
            Err(err) => failures.push(format!("{target_model}：{err}")),
        }
    }

    let vocabulary_ids_value = serde_json::to_value(&vocabulary_ids).map_err(|e| e.to_string())?;
    apply_provider_patch(
        app,
        state,
        &provider_id,
        json!({ "vocabularyIds": vocabulary_ids_value }),
    )?;

    if !failures.is_empty() {
        return Err(format!(
            "部分模型的热词同步失败，已成功的部分不受影响，可重试：{}",
            failures.join("；")
        ));
    }
    Ok(())
}

/// 把当前全局热词同步到所有支持定制的已启用供应商。用户只看到一个按钮，
/// 具体建几份词表、绑定哪个 target_model 是内部实现细节。
#[tauri::command]
pub(crate) async fn customization_sync_providers(
    app: tauri::AppHandle,
    state: tauri::State<'_, RuntimeState>,
) -> Result<CustomizationSyncResponse, String> {
    let hotwords = crate::application::customization::prefs(&state).hotwords;
    if hotwords.is_empty() {
        return Err("请至少添加一个热词".to_string());
    }
    let targets = sync_targets(&state)?;
    if targets.is_empty() {
        return Err("没有已启用且支持热词的供应商".to_string());
    }
    let mut results = Vec::new();
    for (provider_id, display_name) in targets {
        let (ok, message) = match push_to_provider(&app, &state, &provider_id, &hotwords).await {
            Ok(()) => (true, format!("已同步 {} 条热词", hotwords.len())),
            Err(error) => (false, error),
        };
        results.push(ProviderSyncResult {
            provider_id,
            display_name,
            ok,
            message,
        });
    }
    Ok(CustomizationSyncResponse {
        providers: provider_settings_response(read_provider_settings(&state)?),
        results,
    })
}

/// 从指定供应商拉取云端热词，覆盖全局热词列表；上下文模板不受影响。
#[tauri::command]
pub(crate) async fn customization_pull_from_provider(
    app: tauri::AppHandle,
    provider_id: String,
    state: tauri::State<'_, RuntimeState>,
) -> Result<CustomizationPrefs, String> {
    let (provider_id, provider, _) = customization_context_for(&state, &provider_id)?;
    provider.ensure_ready()?;

    let (hotwords, vocabulary_ids) = if provider.is_plugin() {
        (provider.get_hotwords().await?, None)
    } else {
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
        (hotwords, Some(vocabulary_ids))
    };

    if let Some(vocabulary_ids) = vocabulary_ids {
        let value = serde_json::to_value(&vocabulary_ids).map_err(|e| e.to_string())?;
        apply_provider_patch(&app, &state, &provider_id, json!({ "vocabularyIds": value }))?;
    }

    let mut prefs = crate::application::customization::prefs(&state);
    prefs.hotwords = hotwords;
    crate::application::customization::store(&app, &state, &prefs)
}

/// 删除各供应商云端的热词词表并清空本地记录的词表 ID；全局热词列表本身保持不变，
/// 由用户在界面上自行编辑。任一供应商失败都会在结果里单独标出。
#[tauri::command]
pub(crate) async fn customization_clear_providers(
    app: tauri::AppHandle,
    state: tauri::State<'_, RuntimeState>,
) -> Result<CustomizationSyncResponse, String> {
    let targets = sync_targets(&state)?;
    if targets.is_empty() {
        return Err("没有已启用且支持热词的供应商".to_string());
    }
    let mut results = Vec::new();
    for (provider_id, display_name) in targets {
        let (ok, message) = match clear_provider(&app, &state, &provider_id).await {
            Ok(()) => (true, "云端词表已清除".to_string()),
            Err(error) => (false, error),
        };
        results.push(ProviderSyncResult {
            provider_id,
            display_name,
            ok,
            message,
        });
    }
    Ok(CustomizationSyncResponse {
        providers: provider_settings_response(read_provider_settings(&state)?),
        results,
    })
}

async fn clear_provider(
    app: &tauri::AppHandle,
    state: &tauri::State<'_, RuntimeState>,
    provider_id: &str,
) -> Result<(), String> {
    let (provider_id, provider, vocabulary_ids) = customization_context_for(state, provider_id)?;
    if provider.is_plugin() {
        provider.clear_hotwords().await?;
    } else {
        for vocabulary_id in vocabulary_ids.values() {
            provider.delete(vocabulary_id).await?;
        }
    }
    apply_provider_patch(app, state, &provider_id, json!({ "vocabularyIds": {} }))?;
    Ok(())
}
