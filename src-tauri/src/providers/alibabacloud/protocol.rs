use serde::Deserialize;
use std::collections::HashMap;

/// 从 `ProviderProfile.config` 反序列化出的实时识别参数。
#[derive(Clone, Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct FunAsrParams {
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub model: String,
    /// 按 target_model 索引的热词列表 ID（一个模型对应一份词表，见 customization.rs）。
    #[serde(default)]
    pub vocabulary_ids: HashMap<String, String>,
    #[serde(default)]
    pub language_hints: Vec<String>,
    #[serde(default)]
    pub semantic_punctuation_enabled: bool,
    #[serde(default = "default_max_sentence_silence")]
    pub max_sentence_silence: u32,
    #[serde(default)]
    pub multi_threshold_mode_enabled: bool,
    #[serde(default)]
    pub heartbeat: bool,
    #[serde(default)]
    pub speech_noise_threshold: Option<f64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RealtimeAsrFamily {
    DashscopeDuplex,
    QwenRealtime,
}

fn default_max_sentence_silence() -> u32 {
    1300
}

fn default_realtime_model() -> String {
    crate::providers::registry::default_realtime_model().to_string()
}

impl FunAsrParams {
    pub fn realtime_model(&self, model_override: Option<&str>) -> String {
        let candidate = model_override.unwrap_or(&self.model);
        let model = candidate.trim();
        if model.is_empty() {
            default_realtime_model()
        } else {
            model.to_string()
        }
    }
}
