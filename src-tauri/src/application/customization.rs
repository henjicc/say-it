//! 全局热词与上下文。
//!
//! 热词和上下文是同一份「让识别认得专有名词」的用户意图在两种供应商接口上的投影：
//! 一部分模型接受带权重的词表（`supportsVocabulary`），另一部分只接受一段自然语言
//! 上下文（`supportsContext`）。因此这里只维护一份全局数据，由本模块按模型能力
//! 分别渲染，供应商层不再各存一份热词。
//!
//! 上下文模板里的 `{{hotwords}}` 会被替换成热词文本；模板留空时退化为纯热词列表，
//! 保证「只填热词」的用户在只支持上下文的模型上仍然有效。

use crate::providers::capabilities::HotwordEntry;
use crate::state::RuntimeState;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 上下文模板中引用全局热词的变量。与智能处理提示词一样使用英文小写占位符。
pub(crate) const HOTWORDS_PLACEHOLDER: &str = "{{hotwords}}";

/// 单条热词的权重取值范围，与阿里云百炼一致；不支持权重的供应商忽略该字段。
const MIN_WEIGHT: i32 = 1;
const MAX_WEIGHT: i32 = 5;
pub(crate) const DEFAULT_WEIGHT: i32 = 4;

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

/// 一次识别请求实际要用的定制数据：热词按供应商能力直接下发，
/// 上下文是模板渲染并截断后的最终文本。
#[derive(Clone, Debug, Default)]
pub(crate) struct ResolvedCustomization {
    pub(crate) hotwords: Vec<HotwordEntry>,
    pub(crate) context: String,
}

impl ResolvedCustomization {
    pub(crate) fn is_empty(&self) -> bool {
        self.hotwords.is_empty() && self.context.is_empty()
    }
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

pub(crate) fn render_context(prefs: &CustomizationPrefs) -> String {
    let hotwords_text = hotwords_as_text(&prefs.hotwords);
    let template = prefs.context_template.trim();
    let rendered = if template.is_empty() {
        hotwords_text
    } else {
        template.replace(HOTWORDS_PLACEHOLDER, &hotwords_text)
    };
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
    for item in &prefs.hotwords {
        if item.text.trim().is_empty() {
            return Err("热词内容不能为空".into());
        }
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

pub(crate) fn resolve(state: &RuntimeState) -> ResolvedCustomization {
    let prefs = prefs(state);
    ResolvedCustomization {
        context: render_context(&prefs),
        hotwords: prefs.hotwords,
    }
}

/// 0.4.x 之前热词按供应商存在 `profile.config.hotwords`。首次加载时把各供应商的
/// 热词合并进全局配置，避免用户升级后热词凭空消失；`vocabularyIds` 是厂商侧词表 ID，
/// 仍旧留在供应商配置里。
pub(crate) fn migrate_legacy_provider_hotwords(
    settings: &mut crate::application::settings::AppSettings,
    providers: &crate::providers::ProviderSettings,
) {
    let existing: CustomizationPrefs =
        serde_json::from_value(settings.customization_prefs.clone()).unwrap_or_default();
    if !existing.hotwords.is_empty() || !existing.context_template.trim().is_empty() {
        return;
    }
    let mut merged: Vec<HotwordEntry> = Vec::new();
    for profile in &providers.profiles {
        let Some(value) = profile.config.get("hotwords") else {
            continue;
        };
        let Ok(items) = serde_json::from_value::<Vec<HotwordEntry>>(value.clone()) else {
            continue;
        };
        merged.extend(items);
    }
    if merged.is_empty() {
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
    fn empty_template_falls_back_to_plain_hotword_list() {
        let prefs = CustomizationPrefs {
            hotwords: vec![entry("说吧", 4), entry("Fun-ASR", 3)],
            context_template: String::new(),
        };
        assert_eq!(render_context(&prefs), "说吧 Fun-ASR");
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

        migrate_legacy_provider_hotwords(&mut settings, &providers);
        let migrated: CustomizationPrefs =
            serde_json::from_value(settings.customization_prefs.clone()).unwrap();
        assert_eq!(migrated.hotwords, vec![entry("说吧", 3)]);

        // 二次迁移不得覆盖用户后来的编辑结果。
        settings.customization_prefs = serde_json::json!({"hotwords": [], "contextTemplate": "自定义"});
        migrate_legacy_provider_hotwords(&mut settings, &providers);
        assert_eq!(settings.customization_prefs["contextTemplate"], "自定义");
    }
}
