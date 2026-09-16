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

fn configured_vocabulary_ids(config: &Value) -> HashMap<String, String> {
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
            profile
                .capabilities
                .iter()
                .any(|item| item == "customization")
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
    let profile = provider_profile_for_execution(state, provider_id)?;
    let plugin = state
        .plugin_registry
        .lock()
        .map_err(|_| "插件注册表锁失败".to_string())?
        .runtime_for_provider(provider_id)?
        .map(|spec| spec.bind_credentials(state.credentials.clone()));
    let provider = customization_for_with_plugin(&profile, plugin, state.credentials.clone())
        .map_err(|error| error.to_string())?;
    Ok((
        profile.id.clone(),
        provider,
        configured_vocabulary_ids(&profile.config),
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
    Ok(provider_settings_response(
        settings,
        Some(&state.credentials),
    ))
}

/// `push_vocabularies` 用到的词表能力。抽成 trait 只为了让「update 失败后要不要
/// 重建」这段判定能脱离真实凭据与网络被测到；生产路径上唯一的实现就是
/// `CustomizationProvider`。
pub(crate) trait VocabularyApi {
    fn targets(&self) -> &'static [(&'static str, &'static str)];
    fn create(
        &self,
        model: &str,
        prefix: &str,
        words: &[HotwordEntry],
    ) -> impl std::future::Future<Output = Result<String, String>>;
    fn update(
        &self,
        id: &str,
        words: &[HotwordEntry],
    ) -> impl std::future::Future<Output = Result<(), String>>;
    fn list(&self, prefix: &str) -> impl std::future::Future<Output = Result<Vec<String>, String>>;
}

impl VocabularyApi for CustomizationProvider {
    fn targets(&self) -> &'static [(&'static str, &'static str)] {
        CustomizationProvider::targets(self)
    }
    async fn create(
        &self,
        model: &str,
        prefix: &str,
        words: &[HotwordEntry],
    ) -> Result<String, String> {
        CustomizationProvider::create(self, model, prefix, words).await
    }
    async fn update(&self, id: &str, words: &[HotwordEntry]) -> Result<(), String> {
        CustomizationProvider::update(self, id, words).await
    }
    async fn list(&self, prefix: &str) -> Result<Vec<String>, String> {
        CustomizationProvider::list(self, prefix).await
    }
}

/// 把热词写到每个 target_model 各自的词表上，返回「要写回供应商配置的
/// vocabularyIds」与失败描述。
///
/// 两个关键约束（都是以前出过问题的地方）：
///
/// 1. 结果以 `existing` 为起点，而不是一份空 map。否则只要同步失败，调用方仍会
///    把返回值整份覆盖写回配置，一次网络抖动就把全部云端词表绑定刷没了。
/// 2. update 失败后先用 `list(prefix)` 确认词表是否真的不在云端了，确认不在才重建。
///    盲目回退为 create 会把限流、服务端故障等瞬时错误也当成「ID 失效」，在供应商
///    侧不断产生孤儿词表、重复占用配额。
async fn push_vocabularies<V: VocabularyApi>(
    provider: &V,
    existing: &HashMap<String, String>,
    hotwords: &[HotwordEntry],
) -> (HashMap<String, String>, Vec<String>) {
    let mut vocabulary_ids = existing.clone();
    let mut failures = Vec::new();
    for (target_model, prefix) in provider.targets() {
        let current = existing.get(*target_model).cloned().unwrap_or_default();
        let result = if current.is_empty() {
            provider.create(target_model, prefix, hotwords).await
        } else {
            match provider.update(&current, hotwords).await {
                Ok(()) => Ok(current.clone()),
                Err(update_error) => match provider.list(prefix).await {
                    // 词表确实已经不在了（例如在阿里云控制台被删掉），重建。
                    Ok(ids) if !ids.iter().any(|id| id == &current) => {
                        provider.create(target_model, prefix, hotwords).await
                    }
                    // 词表还在，说明 update 是瞬时失败；保留原 ID，下次重试即可。
                    Ok(_) => Err(update_error),
                    Err(list_error) => {
                        Err(format!("{update_error}（确认词表是否仍存在也失败：{list_error}）"))
                    }
                },
            }
        };
        match result {
            Ok(id) => {
                vocabulary_ids.insert(target_model.to_string(), id);
            }
            Err(err) => failures.push(format!("{target_model}：{err}")),
        }
    }
    (vocabulary_ids, failures)
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

    let (vocabulary_ids, failures) = push_vocabularies(&provider, &existing_ids, hotwords).await;

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
    let targets = sync_targets(&state)?;
    if targets.is_empty() {
        return Err("没有已启用且支持热词的供应商".to_string());
    }
    let mut results = Vec::new();
    for (provider_id, display_name) in targets {
        // 热词被删光是一次**清空**，不是「没东西可推」。此前这里直接报错要求先去
        // 添加热词，于是供应商侧的旧词表和 vocabularyIds 会一直留着，
        // 识别时照样下发——用户在界面上把热词删光了，实际效果完全没变，而且除了手动
        // 点「清除云端词表」之外没有任何入口能纠正。
        let (ok, message) = if hotwords.is_empty() {
            match clear_provider(&app, &state, &provider_id).await {
                Ok(()) => (true, "热词已清空，云端词表同步清除".to_string()),
                Err(error) => (false, error),
            }
        } else {
            match push_to_provider(&app, &state, &provider_id, &hotwords).await {
                Ok(()) => (true, format!("已同步 {} 条热词", hotwords.len())),
                Err(error) => (false, error),
            }
        };
        results.push(ProviderSyncResult {
            provider_id,
            display_name,
            ok,
            message,
        });
    }
    Ok(CustomizationSyncResponse {
        providers: provider_settings_response(
            read_provider_settings(&state)?,
            Some(&state.credentials),
        ),
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
        apply_provider_patch(
            &app,
            &state,
            &provider_id,
            json!({ "vocabularyIds": value }),
        )?;
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
        providers: provider_settings_response(
            read_provider_settings(&state)?,
            Some(&state.credentials),
        ),
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    const TARGETS: &[(&str, &str)] = &[("model-a", "prefa"), ("model-b", "prefb")];

    /// 按词表 ID 编排结果的假供应商。内部用 `RefCell`：测试里只有单线程、
    /// 且 await 点上不持有借用。
    struct FakeProvider {
        /// 云端真实存在的词表 ID。
        cloud: RefCell<Vec<String>>,
        /// 指定词表 ID 的 update 要报的错误。
        update_error: Option<(String, String)>,
        list_error: Option<String>,
        created: RefCell<Vec<String>>,
    }

    impl FakeProvider {
        fn new(cloud: &[&str]) -> Self {
            Self {
                cloud: RefCell::new(cloud.iter().map(|id| id.to_string()).collect()),
                update_error: None,
                list_error: None,
                created: RefCell::new(Vec::new()),
            }
        }
        fn failing_update(mut self, id: &str, error: &str) -> Self {
            self.update_error = Some((id.to_string(), error.to_string()));
            self
        }
        fn failing_list(mut self, error: &str) -> Self {
            self.list_error = Some(error.to_string());
            self
        }
    }

    impl VocabularyApi for FakeProvider {
        fn targets(&self) -> &'static [(&'static str, &'static str)] {
            TARGETS
        }
        async fn create(
            &self,
            model: &str,
            _prefix: &str,
            _words: &[HotwordEntry],
        ) -> Result<String, String> {
            let id = format!("new-{model}");
            self.created.borrow_mut().push(id.clone());
            self.cloud.borrow_mut().push(id.clone());
            Ok(id)
        }
        async fn update(&self, id: &str, _words: &[HotwordEntry]) -> Result<(), String> {
            match &self.update_error {
                Some((target, error)) if target == id => Err(error.clone()),
                _ => Ok(()),
            }
        }
        async fn list(&self, prefix: &str) -> Result<Vec<String>, String> {
            if let Some(error) = &self.list_error {
                return Err(error.clone());
            }
            Ok(self
                .cloud
                .borrow()
                .iter()
                .filter(|id| id.starts_with(prefix))
                .cloned()
                .collect())
        }
    }

    fn ids(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn hotwords() -> Vec<HotwordEntry> {
        vec![HotwordEntry {
            text: "说吧".into(),
            weight: 4,
        }]
    }

    /// 一个模型同步失败，不得连带把其他模型的绑定刷没。
    ///
    /// 以前结果 map 从空开始、只塞成功项，而调用方把它**整份覆盖**写回配置：
    /// 一次网络抖动就把失败模型的旧词表 ID 永久丢掉，识别时热词直接不再下发。
    #[tokio::test]
    async fn failing_model_keeps_its_existing_vocabulary_id() {
        let provider = FakeProvider::new(&["prefa-1", "prefb-2"])
            .failing_update("prefb-2", "热词接口返回 429：限流");
        let existing = ids(&[("model-a", "prefa-1"), ("model-b", "prefb-2")]);

        let (next, failures) = push_vocabularies(&provider, &existing, &hotwords()).await;

        assert_eq!(failures.len(), 1, "model-b 应该报失败：{failures:?}");
        assert_eq!(
            next.get("model-b").map(String::as_str),
            Some("prefb-2"),
            "失败模型的旧词表 ID 必须原样保留"
        );
        assert_eq!(next.get("model-a").map(String::as_str), Some("prefa-1"));
    }

    /// update 失败不能盲目回退为 create。
    ///
    /// 词表明明还在云端，只是这次请求被限流/服务端报错。以前 `Err(_) => create`
    /// 会在供应商侧不断新建孤儿词表，重复占用配额。
    #[tokio::test]
    async fn transient_update_failure_does_not_create_an_orphan_vocabulary() {
        let provider = FakeProvider::new(&["prefa-1", "prefb-2"])
            .failing_update("prefb-2", "热词接口返回 500：内部错误");
        let existing = ids(&[("model-a", "prefa-1"), ("model-b", "prefb-2")]);

        let (next, failures) = push_vocabularies(&provider, &existing, &hotwords()).await;

        assert!(
            provider.created.borrow().is_empty(),
            "词表仍在云端时不得新建：{:?}",
            provider.created.borrow()
        );
        assert_eq!(next.get("model-b").map(String::as_str), Some("prefb-2"));
        assert!(failures[0].contains("500"), "{failures:?}");
    }

    /// 词表真的被在控制台删掉时，仍然要重建。
    #[tokio::test]
    async fn missing_vocabulary_is_recreated() {
        let provider = FakeProvider::new(&["prefa-1"]).failing_update("prefb-2", "热词接口返回 400：词表不存在");
        let existing = ids(&[("model-a", "prefa-1"), ("model-b", "prefb-2")]);

        let (next, failures) = push_vocabularies(&provider, &existing, &hotwords()).await;

        assert!(failures.is_empty(), "{failures:?}");
        assert_eq!(next.get("model-b").map(String::as_str), Some("new-model-b"));
    }

    /// 连「词表还在不在」都问不出来时，宁可报错也不猜。
    #[tokio::test]
    async fn unknown_vocabulary_state_reports_instead_of_recreating() {
        let provider = FakeProvider::new(&["prefa-1", "prefb-2"])
            .failing_update("prefb-2", "请求热词接口失败：超时")
            .failing_list("请求热词接口失败：超时");
        let existing = ids(&[("model-a", "prefa-1"), ("model-b", "prefb-2")]);

        let (next, failures) = push_vocabularies(&provider, &existing, &hotwords()).await;

        assert!(provider.created.borrow().is_empty());
        assert_eq!(next.get("model-b").map(String::as_str), Some("prefb-2"));
        assert_eq!(failures.len(), 1, "{failures:?}");
    }

    /// 没有旧 ID 的模型正常新建并记下来。
    #[tokio::test]
    async fn model_without_existing_id_creates_one() {
        let provider = FakeProvider::new(&[]);
        let (next, failures) = push_vocabularies(&provider, &HashMap::new(), &hotwords()).await;
        assert!(failures.is_empty(), "{failures:?}");
        assert_eq!(next.get("model-a").map(String::as_str), Some("new-model-a"));
        assert_eq!(next.get("model-b").map(String::as_str), Some("new-model-b"));
    }

    /// 热词被删光是一次**清空**，不是「没东西可推」。
    ///
    /// 同步命令此前对空热词直接返回「请至少添加一个热词」，于是供应商侧的旧词表和
    /// `vocabularyIds` 会一直留着并在识别时继续下发——用户以为删干净了，实际毫无变化。
    /// 这条路径要真跑起来需要真实的供应商凭据与网络，只能做源码契约校验。
    #[test]
    fn empty_hotwords_clear_the_provider_instead_of_being_rejected() {
        // 归一化行尾：按 core.autocrlf 检出时工作区是 CRLF，含 \n 的切片会失配。
        let source = include_str!("customization.rs").replace("\r\n", "\n");
        let body = &source[..source
            .find("#[cfg(test)]")
            .expect("customization.rs 必须有测试模块标记")];
        let start = body
            .find("pub(crate) async fn customization_sync_providers")
            .expect("同步命令必须仍然存在");
        let command = &body[start..];
        let command = &command[..command.find("\n}\n").expect("函数体未闭合")];

        assert!(
            !command.contains("请至少添加一个热词"),
            "空热词不得被拒绝，它意味着要清空云端词表"
        );
        assert!(
            command.contains("hotwords.is_empty()") && command.contains("clear_provider("),
            "空热词必须走 clear_provider 清理云端词表与 vocabularyIds"
        );
    }
}
