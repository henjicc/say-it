use serde::Serialize;
use std::collections::HashSet;

use crate::commands::common::{provider_settings_response, read_provider_settings};
use crate::providers::registry::{self, FileTranscriptionRoute};
use crate::providers::plugin::PluginRegistry;
use crate::providers::ProviderSettingsResponse;
use crate::state::RuntimeState;

pub const CATALOG_VERSION: u32 = 4;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelCatalogItem {
    pub id: String,
    pub label: String,
    pub provider_id: String,
    pub category: String,
    pub protocol: String,
    pub scenes: Vec<String>,
    pub supports_vocabulary: bool,
    /// 已按「未声明即不支持」解析，前端直接消费。
    pub supports_context: bool,
    pub supports_alignment_timestamps: bool,
    /// 已按 `category` 兜底解析，前端直接消费，不再感知"未声明"状态。
    pub emits_partial_results: bool,
    pub is_default_realtime: bool,
    pub is_default_file: bool,
    pub is_qwen_realtime: bool,
    pub is_qwen_file: bool,
    pub is_qwen_short_audio_file: bool,
    pub is_funasr_flash_file: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelCatalog {
    pub version: u32,
    pub default_realtime_model: String,
    pub default_file_model: String,
    pub models: Vec<ModelCatalogItem>,
    pub providers: ProviderSettingsResponse,
}

pub fn build_catalog(providers: ProviderSettingsResponse, plugins: &PluginRegistry) -> ModelCatalog {
    let enabled_provider_ids = providers
        .profiles
        .iter()
        .filter(|provider| provider.enabled)
        .map(|provider| provider.id.as_str())
        .collect::<HashSet<_>>();
    let models = registry::models()
        .iter()
        .chain(plugins.models())
        .filter(|model| enabled_provider_ids.contains(model.provider_id.as_str()))
        .map(|model| {
            let route = registry::file_transcription_route(&model.id);
            ModelCatalogItem {
                id: model.id.clone(),
                label: model.label.clone(),
                provider_id: model.provider_id.clone(),
                category: model.category.clone(),
                protocol: model.protocol.clone(),
                scenes: model.scenes.clone(),
                supports_vocabulary: model.supports_vocabulary,
                supports_context: model.supports_context.unwrap_or(false),
                supports_alignment_timestamps: model.supports_alignment_timestamps,
                emits_partial_results: model.emits_partial_results(),
                is_default_realtime: model.is_default_realtime,
                is_default_file: model.is_default_file,
                is_qwen_realtime: matches!(
                    registry::realtime_asr_family(&model.id),
                    crate::providers::alibabacloud::RealtimeAsrFamily::QwenRealtime
                ),
                is_qwen_file: model.id.starts_with("qwen3-asr-flash-filetrans"),
                is_qwen_short_audio_file: route == FileTranscriptionRoute::SyncQwen,
                is_funasr_flash_file: route == FileTranscriptionRoute::SyncFunAsrFlash,
            }
        })
        .collect();
    ModelCatalog {
        version: CATALOG_VERSION,
        default_realtime_model: registry::default_realtime_model().into(),
        default_file_model: registry::default_file_model().into(),
        models,
        providers,
    }
}

#[tauri::command]
pub(crate) fn get_model_catalog(
    state: tauri::State<'_, RuntimeState>,
) -> Result<ModelCatalog, String> {
    let plugins = state.plugin_registry.lock().map_err(|_| "插件注册表锁失败")?;
    Ok(build_catalog(
        provider_settings_response(read_provider_settings(&state)?),
        &plugins,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::{normalize_settings, ProviderSettings};

    #[test]
    fn catalog_is_complete_and_defaults_exist() {
        let catalog = build_catalog(
            provider_settings_response(normalize_settings(ProviderSettings::default())),
            &PluginRegistry::default(),
        );
        assert_eq!(catalog.models.len(), 9);
        assert!(catalog
            .models
            .iter()
            .any(|m| m.id == catalog.default_realtime_model && m.is_default_realtime));
        assert!(catalog
            .models
            .iter()
            .any(|m| m.id == catalog.default_file_model && m.is_default_file));
        assert!(catalog.models.iter().all(|m| !m.scenes.is_empty()));
    }

    #[test]
    fn emits_partial_results_falls_back_to_category() {
        // 内置模型都没有显式声明，必须按 category 推导出与改动前一致的行为，
        // 否则旧插件清单升级后下拉标注会集体错位。
        let catalog = build_catalog(
            provider_settings_response(normalize_settings(ProviderSettings::default())),
            &PluginRegistry::default(),
        );
        for model in &catalog.models {
            assert_eq!(
                model.emits_partial_results,
                model.category == "realtime",
                "模型 {} 的中间结果推导与 category 不一致",
                model.id
            );
        }
    }

    #[test]
    fn explicit_emits_partial_results_overrides_category() {
        // VAD 分段整句模型：category 是 realtime，但必须能声明自己没有中间结果。
        let model: registry::ModelInfo = serde_json::from_value(serde_json::json!({
            "id": "x", "label": "X", "providerId": "p", "category": "realtime",
            "protocol": "local-sherpa-offline", "supportsVocabulary": false,
            "supportsAlignmentTimestamps": false, "emitsPartialResults": false,
            "scenes": ["dictationRealtime"], "isDefaultRealtime": false, "isDefaultFile": false
        }))
        .unwrap();
        assert!(!model.emits_partial_results());
    }

    #[test]
    fn offline_engine_has_no_partials_even_without_declaration() {
        // 字段上线前打包安装的模型包清单里没有 emitsPartialResults，此时不能退回
        // category 推导，否则整句模型会被当成真流式，下拉里看不出区别。
        let model: registry::ModelInfo = serde_json::from_value(serde_json::json!({
            "id": "local-sensevoice-sentence-int8", "label": "SenseVoice", "providerId": "p",
            "category": "realtime", "protocol": "local-sherpa-offline",
            "supportsVocabulary": false, "supportsAlignmentTimestamps": false,
            "scenes": ["dictationRealtime"], "isDefaultRealtime": false, "isDefaultFile": false
        }))
        .unwrap();
        assert!(!model.emits_partial_results());
    }

    #[test]
    fn disabled_or_missing_default_uses_enabled_capable_provider() {
        let mut settings = ProviderSettings::default();
        settings.profiles[0].enabled = false;
        settings.defaults.asr = "missing".into();
        settings.profiles.push(crate::providers::ProviderProfile {
            id: "fallback".into(),
            kind: "test".into(),
            display_name: "Fallback".into(),
            auth_kind: "none".into(),
            capabilities: vec!["asr".into()],
            enabled: true,
            config: serde_json::json!({}),
            config_fields: vec![],
            actions: vec![],
        });
        let catalog = build_catalog(
            provider_settings_response(normalize_settings(settings)),
            &PluginRegistry::default(),
        );
        let effective = catalog
            .providers
            .profiles
            .iter()
            .find(|p| p.effective_capabilities.iter().any(|c| c == "asr"));
        assert_eq!(effective.map(|p| p.id.as_str()), Some("funasr"));
    }
}
