//! 全局热词与上下文。
//!
//! 热词和上下文是同一份「让识别认得专有名词」的用户意图在两种供应商接口上的投影：
//! 一部分模型接受带权重的词表（`supportsVocabulary`），另一部分只接受一段自然语言
//! 上下文（`supportsContext`）。因此这里只维护一份全局数据，由本模块按模型能力
//! 分别渲染，供应商层不再各存一份热词。
//!
//! 上下文完全由模板决定：模板留空就不下发上下文，需要带上热词时由用户在模板里显式
//! 插入 `{{hotwords}}` 变量。热词不会被隐式塞进上下文。

use crate::providers::capabilities::HotwordEntry;
use crate::providers::RequestCustomization;
use crate::state::RuntimeState;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 上下文模板中引用全局热词的变量。与智能处理提示词一样使用英文小写占位符。
pub(crate) const HOTWORDS_PLACEHOLDER: &str = "{{hotwords}}";

/// 单条热词的权重取值范围，与阿里云百炼一致；不支持权重的供应商忽略该字段。
const MIN_WEIGHT: i32 = 1;
const MAX_WEIGHT: i32 = 5;

const MAX_HOTWORDS: usize = 500;
const MAX_HOTWORD_CHARS: usize = 64;
const MAX_CONTEXT_TEMPLATE_CHARS: usize = 4_000;

/// 上下文送给供应商时的字符上限。阿里云百炼文档规定单轮上下文不超过 400 字符，
/// 超出部分服务端会静默从末尾截断；这里主动截断以便日志和预览与实际生效内容一致。
pub(crate) const MAX_CONTEXT_CHARS: usize = 400;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct CustomizationPrefs {
    pub(crate) hotwords: Vec<HotwordEntry>,
    pub(crate) context_template: String,
}

/// 热词在上下文里的呈现形式：空格分隔的纯词列表。
/// 阿里云文档说明上下文按词表匹配生效，`text` 必须包含音频里的原词，不带权重。
fn hotwords_as_text(hotwords: &[HotwordEntry]) -> String {
    hotwords
        .iter()
        .map(|item| item.text.trim())
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    text.chars().take(max_chars).collect()
}

/// 上下文完全由模板决定：模板留空就不下发上下文，模板里没有 `{{hotwords}}` 就不带热词。
/// 热词要不要出现在上下文里是用户的显式选择，不做隐式合并。
pub(crate) fn render_context(prefs: &CustomizationPrefs) -> String {
    let template = prefs.context_template.trim();
    if template.is_empty() {
        return String::new();
    }
    let rendered = template.replace(HOTWORDS_PLACEHOLDER, &hotwords_as_text(&prefs.hotwords));
    truncate_chars(rendered.trim(), MAX_CONTEXT_CHARS)
}

pub(crate) fn normalize(prefs: &CustomizationPrefs) -> CustomizationPrefs {
    let mut seen = std::collections::HashSet::new();
    let hotwords = prefs
        .hotwords
        .iter()
        .filter_map(|item| {
            let text = item.text.trim();
            if text.is_empty() || !seen.insert(text.to_string()) {
                return None;
            }
            Some(HotwordEntry {
                text: text.to_string(),
                weight: item.weight.clamp(MIN_WEIGHT, MAX_WEIGHT),
            })
        })
        .collect();
    CustomizationPrefs {
        hotwords,
        context_template: prefs.context_template.trim().to_string(),
    }
}

pub(crate) fn validate_customization_settings_value(value: &Value) -> Result<(), String> {
    let prefs: CustomizationPrefs = serde_json::from_value(value.clone())
        .map_err(|error| format!("热词与上下文配置格式错误：{error}"))?;
    if prefs.hotwords.len() > MAX_HOTWORDS {
        return Err(format!("热词数量不能超过 {MAX_HOTWORDS} 条"));
    }
    // 允许空热词：界面上「添加热词」先插入一条待填写的空行，落库后由 normalize 剔除。
    for item in &prefs.hotwords {
        if item.text.chars().count() > MAX_HOTWORD_CHARS {
            return Err(format!("单条热词不能超过 {MAX_HOTWORD_CHARS} 个字符"));
        }
        if !(MIN_WEIGHT..=MAX_WEIGHT).contains(&item.weight) {
            return Err(format!("热词权重必须在 {MIN_WEIGHT} 到 {MAX_WEIGHT} 之间"));
        }
    }
    if prefs.context_template.chars().count() > MAX_CONTEXT_TEMPLATE_CHARS {
        return Err(format!(
            "上下文模板不能超过 {MAX_CONTEXT_TEMPLATE_CHARS} 个字符"
        ));
    }
    Ok(())
}

pub(crate) fn prefs(state: &RuntimeState) -> CustomizationPrefs {
    state
        .app_settings
        .lock()
        .ok()
        .map(|settings| settings.customization_prefs.clone())
        .and_then(|value| serde_json::from_value::<CustomizationPrefs>(value).ok())
        .map(|prefs| normalize(&prefs))
        .unwrap_or_default()
}

/// 模型声明的定制能力。内置模型查注册表，插件与模型包查插件注册表，
/// 两处都查不到时按「都不支持」处理，避免向不认识这些字段的模型塞内容。
fn capabilities_of(state: &RuntimeState, model: &str) -> (bool, bool) {
    if let Some(info) = crate::providers::registry::model_info(model) {
        return (
            info.supports_vocabulary,
            info.supports_context.unwrap_or(false),
        );
    }
    if let Ok(registry) = state.plugin_registry.lock() {
        if let Some(info) = registry.model(model) {
            return (
                info.supports_vocabulary,
                info.supports_context.unwrap_or(false),
            );
        }
    }
    (false, false)
}

/// 按模型声明裁剪全局配置：支持热词的模型才拿到 `hotwords`，
/// 支持上下文的模型才拿到渲染后的 `context`。两者互不影响，可同时下发。
pub(crate) fn resolve_for_model(state: &RuntimeState, model: &str) -> RequestCustomization {
    let (supports_vocabulary, supports_context) = capabilities_of(state, model);
    if !supports_vocabulary && !supports_context {
        return RequestCustomization::default();
    }
    let prefs = prefs(state);
    RequestCustomization {
        context: if supports_context {
            render_context(&prefs)
        } else {
            String::new()
        },
        hotwords: if supports_vocabulary {
            prefs.hotwords
        } else {
            vec![]
        },
    }
}

/// 写入全局热词与上下文并落盘。走 `update_app_settings` 以复用同一条校验与持久化路径。
pub(crate) fn store(
    app: &tauri::AppHandle,
    state: &tauri::State<'_, RuntimeState>,
    prefs: &CustomizationPrefs,
) -> Result<CustomizationPrefs, String> {
    let normalized = normalize(prefs);
    let value = serde_json::to_value(&normalized).map_err(|error| error.to_string())?;
    crate::application::settings::update_app_settings(
        app.clone(),
        state.clone(),
        "customization".into(),
        value,
    )?;
    Ok(normalized)
}

/// 0.4.x 之前热词按供应商存在 `profile.config.hotwords`。加载时把这份数据搬进全局配置：
/// 全局还是空的就合并写入，避免用户升级后热词凭空消失；无论是否写入都从供应商配置里
/// 移除该键，防止留下两份互相矛盾的热词。`vocabularyIds` 是厂商侧词表 ID，仍留在供应商配置。
pub(crate) fn migrate_legacy_provider_hotwords(
    settings: &mut crate::application::settings::AppSettings,
    providers: &mut crate::providers::ProviderSettings,
) {
    let mut merged: Vec<HotwordEntry> = Vec::new();
    for profile in &mut providers.profiles {
        let Some(config) = profile.config.as_object_mut() else {
            continue;
        };
        let Some(value) = config.remove("hotwords") else {
            continue;
        };
        if let Ok(items) = serde_json::from_value::<Vec<HotwordEntry>>(value) {
            merged.extend(items);
        }
    }
    if merged.is_empty() {
        return;
    }
    let existing: CustomizationPrefs =
        serde_json::from_value(settings.customization_prefs.clone()).unwrap_or_default();
    if !existing.hotwords.is_empty() || !existing.context_template.trim().is_empty() {
        return;
    }
    let normalized = normalize(&CustomizationPrefs {
        hotwords: merged,
        context_template: String::new(),
    });
    if let Ok(value) = serde_json::to_value(&normalized) {
        settings.customization_prefs = value;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(text: &str, weight: i32) -> HotwordEntry {
        HotwordEntry {
            text: text.into(),
            weight,
        }
    }

    #[test]
    fn hotwords_only_reach_context_through_the_placeholder() {
        let hotwords = vec![entry("说吧", 4), entry("Fun-ASR", 3)];
        // 模板留空：不下发上下文。
        assert_eq!(
            render_context(&CustomizationPrefs {
                hotwords: hotwords.clone(),
                context_template: String::new(),
            }),
            ""
        );
        // 模板不含变量：只用模板文本，不隐式塞入热词。
        assert_eq!(
            render_context(&CustomizationPrefs {
                hotwords,
                context_template: "一场技术分享".into(),
            }),
            "一场技术分享"
        );
    }

    #[test]
    fn template_replaces_hotwords_placeholder() {
        let prefs = CustomizationPrefs {
            hotwords: vec![entry("Kubernetes", 4)],
            context_template: "本次会议涉及的术语：{{hotwords}}。".into(),
        };
        assert_eq!(render_context(&prefs), "本次会议涉及的术语：Kubernetes。");
    }

    #[test]
    fn rendered_context_is_truncated_to_provider_limit() {
        let prefs = CustomizationPrefs {
            hotwords: vec![],
            context_template: "词".repeat(MAX_CONTEXT_CHARS + 50),
        };
        assert_eq!(render_context(&prefs).chars().count(), MAX_CONTEXT_CHARS);
    }

    #[test]
    fn normalize_trims_deduplicates_and_clamps_weight() {
        let normalized = normalize(&CustomizationPrefs {
            hotwords: vec![entry(" 说吧 ", 9), entry("说吧", 2), entry("  ", 3)],
            context_template: "  模板  ".into(),
        });
        assert_eq!(normalized.hotwords.len(), 1);
        assert_eq!(normalized.hotwords[0].text, "说吧");
        assert_eq!(normalized.hotwords[0].weight, MAX_WEIGHT);
        assert_eq!(normalized.context_template, "模板");
    }

    #[test]
    fn validation_rejects_out_of_range_weight() {
        let value = serde_json::json!({"hotwords": [{"text": "说吧", "weight": 9}]});
        assert!(validate_customization_settings_value(&value)
            .unwrap_err()
            .contains("权重"));
    }

    #[test]
    fn legacy_provider_hotwords_are_merged_once() {
        let mut settings = crate::application::settings::AppSettings::default();
        let mut providers = crate::providers::ProviderSettings::default();
        let mut profile = crate::providers::funasr_profile();
        profile.config["hotwords"] =
            serde_json::json!([{"text": "说吧", "weight": 3}, {"text": "说吧", "weight": 5}]);
        providers.profiles = vec![profile];

        migrate_legacy_provider_hotwords(&mut settings, &mut providers);
        let migrated: CustomizationPrefs =
            serde_json::from_value(settings.customization_prefs.clone()).unwrap();
        assert_eq!(migrated.hotwords, vec![entry("说吧", 3)]);
        // 搬走后供应商配置里不得再留一份热词。
        assert!(providers.profiles[0].config.get("hotwords").is_none());

        // 二次迁移不得覆盖用户后来的编辑结果。
        settings.customization_prefs = serde_json::json!({"hotwords": [], "contextTemplate": "自定义"});
        providers.profiles[0].config["hotwords"] = serde_json::json!([{"text": "残留", "weight": 3}]);
        migrate_legacy_provider_hotwords(&mut settings, &mut providers);
        assert_eq!(settings.customization_prefs["contextTemplate"], "自定义");
        assert!(providers.profiles[0].config.get("hotwords").is_none());
    }
}
