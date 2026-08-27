use crate::providers::capabilities::translation_for_with_plugin;
use crate::providers::capabilities::TranslationProvider;
use crate::providers::default_provider_id;
use crate::state::RuntimeState;

pub(crate) async fn translate_text(
    state: &RuntimeState,
    model: &str,
    text: &str,
    source_language: &str,
    target_language: &str,
) -> Result<String, String> {
    if text.trim().is_empty() {
        return Ok(String::new());
    }
    let provider = resolve_provider(state, model)?;
    provider
        .translate_streaming(model, text, source_language, target_language, |_| {})
        .await
}

pub(crate) fn validate_available(state: &RuntimeState, model: &str) -> Result<(), String> {
    resolve_provider(state, model).map(|_| ())
}

pub(crate) fn resolve_provider(
    state: &RuntimeState,
    model: &str,
) -> Result<TranslationProvider, String> {
    if model.trim().is_empty() || model == "none" {
        return Err("请先在“智能助手”中选择翻译模型".into());
    }
    let plugin_provider = state
        .plugin_registry
        .lock()
        .map_err(|_| "插件注册表锁失败")?
        .provider_id_for_model(model);
    let settings = state
        .providers
        .lock()
        .map_err(|_| "供应商配置锁失败")?
        .clone();
    let provider_id =
        plugin_provider.unwrap_or_else(|| default_provider_id(&settings, "translation"));
    let profile = crate::commands::common::provider_profile_for_execution(state, &provider_id)?;
    if !profile.enabled {
        return Err(format!("翻译供应商 {provider_id} 不存在或未启用"));
    }
    let plugin = state
        .plugin_registry
        .lock()
        .map_err(|_| "插件注册表锁失败")?
        .runtime_for_provider(&provider_id)?
        .map(|spec| spec.bind_credentials(state.credentials.clone()));
    translation_for_with_plugin(&profile, plugin).map_err(|error| error.to_string())
}
