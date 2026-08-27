use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptionParams {
    #[serde(default = "super::default_transcription_model")]
    pub model: String,
    #[serde(default)]
    pub language_hints: Vec<String>,
    #[serde(default)]
    pub diarization_enabled: Option<bool>,
    #[serde(default)]
    pub speaker_count: Option<u32>,
    #[serde(default)]
    pub channel_id: Option<Value>,
    #[serde(default)]
    pub special_word_filter: String,
}

impl Default for TranscriptionParams {
    fn default() -> Self {
        Self {
            model: super::default_transcription_model(),
            language_hints: Vec::new(),
            diarization_enabled: None,
            speaker_count: None,
            channel_id: None,
            special_word_filter: String::new(),
        }
    }
}

impl TranscriptionParams {
    pub fn model_id(&self) -> String {
        let model = self.model.trim();
        if model.is_empty() {
            super::default_transcription_model()
        } else {
            model.to_string()
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptionResult {
    pub duration_ms: Option<u64>,
    pub transcripts: Vec<TranscriptionTranscript>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptionTranscript {
    #[serde(default, alias = "channel_id")]
    pub channel_id: Option<Value>,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub sentences: Vec<TranscriptionSentence>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptionSentence {
    #[serde(default, alias = "begin_time")]
    pub begin_time: u64,
    #[serde(default, alias = "end_time")]
    pub end_time: u64,
    #[serde(default)]
    pub text: String,
    #[serde(default, alias = "sentence_id")]
    pub sentence_id: Option<Value>,
    #[serde(default, alias = "speaker_id")]
    pub speaker_id: Option<Value>,
    #[serde(default)]
    pub words: Vec<TranscriptionWord>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptionWord {
    #[serde(default, alias = "begin_time")]
    pub begin_time: u64,
    #[serde(default, alias = "end_time")]
    pub end_time: u64,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub punctuation: Option<String>,
}
