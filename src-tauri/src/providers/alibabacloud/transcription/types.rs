use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

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

    pub(super) fn parameters_value(
        &self,
        family: super::TranscriptionModelFamily,
        vocabulary_id: &str,
    ) -> Value {
        use super::TranscriptionModelFamily;

        let mut parameters = Map::new();
        if matches!(
            family,
            TranscriptionModelFamily::FunAsr | TranscriptionModelFamily::Paraformer
        ) && !vocabulary_id.trim().is_empty()
        {
            parameters.insert("vocabulary_id".to_string(), json!(vocabulary_id.trim()));
        }
        let language_hints = self
            .language_hints
            .iter()
            .map(|hint| hint.trim())
            .filter(|hint| !hint.is_empty())
            .collect::<Vec<_>>();
        if matches!(
            family,
            TranscriptionModelFamily::FunAsr | TranscriptionModelFamily::Paraformer
        ) && !language_hints.is_empty()
        {
            parameters.insert("language_hints".to_string(), json!(language_hints));
        } else if family == TranscriptionModelFamily::QwenFiletrans && language_hints.len() == 1 {
            parameters.insert("language".to_string(), json!(language_hints[0]));
        }
        if matches!(
            family,
            TranscriptionModelFamily::FunAsr | TranscriptionModelFamily::Paraformer
        ) {
            if let Some(enabled) = self.diarization_enabled {
                parameters.insert("diarization_enabled".to_string(), json!(enabled));
            }
            if let Some(count) = self.speaker_count.filter(|count| *count > 0) {
                parameters.insert("speaker_count".to_string(), json!(count));
            }
        }
        if let Some(channel_id) = &self.channel_id {
            if !channel_id.is_null() {
                parameters.insert("channel_id".to_string(), channel_id.clone());
            }
        }
        if family == TranscriptionModelFamily::QwenFiletrans {
            parameters.insert("enable_words".to_string(), json!(true));
            parameters.insert("enable_itn".to_string(), json!(false));
        }
        if matches!(
            family,
            TranscriptionModelFamily::FunAsr | TranscriptionModelFamily::Paraformer
        ) && !self.special_word_filter.trim().is_empty()
        {
            parameters.insert(
                "special_word_filter".to_string(),
                json!(self.special_word_filter.trim()),
            );
        }
        Value::Object(parameters)
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptionTaskStatus {
    pub task_status: String,
    pub result: Option<TranscriptionTaskResult>,
    pub code: Option<String>,
    pub message: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptionTaskResult {
    #[serde(default, alias = "subtask_status")]
    pub subtask_status: Option<String>,
    #[serde(default, alias = "transcription_url")]
    pub transcription_url: Option<String>,
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
}

impl TranscriptionTaskStatus {
    pub fn successful_transcription_url(&self) -> Result<String, String> {
        let Some(result) = &self.result else {
            return Err("录音识别任务成功但响应缺少结果地址".to_string());
        };
        if result
            .subtask_status
            .as_deref()
            .map(|status| status.eq_ignore_ascii_case("FAILED"))
            .unwrap_or(false)
        {
            return Err(super::http::format_task_error(
                "录音识别子任务失败",
                result.code.as_deref(),
                result.message.as_deref(),
            ));
        }
        result
            .transcription_url
            .as_deref()
            .filter(|url| !url.trim().is_empty())
            .map(|url| url.trim().to_string())
            .ok_or_else(|| "录音识别任务成功但响应缺少 transcription_url".to_string())
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

#[derive(Debug, Deserialize)]
pub(super) struct SubmitResponse {
    pub(super) output: SubmitOutput,
}

#[derive(Debug, Deserialize)]
pub(super) struct SubmitOutput {
    pub(super) task_id: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct TaskResponse {
    pub(super) output: TaskOutput,
}

#[derive(Debug, Deserialize)]
pub(super) struct TaskOutput {
    pub(super) task_status: String,
    #[serde(default)]
    pub(super) result: Option<TranscriptionTaskResult>,
    #[serde(default)]
    pub(super) results: Vec<TranscriptionTaskResult>,
    #[serde(default)]
    pub(super) code: Option<String>,
    #[serde(default)]
    pub(super) message: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct RawTranscriptionResult {
    #[serde(default)]
    pub(super) properties: TranscriptionProperties,
    #[serde(default)]
    pub(super) transcripts: Vec<TranscriptionTranscript>,
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct TranscriptionProperties {
    #[serde(default, alias = "originalDurationInMilliseconds")]
    pub(super) original_duration_in_milliseconds: Option<u64>,
}
