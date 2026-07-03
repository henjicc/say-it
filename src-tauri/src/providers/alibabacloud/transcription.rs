use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

const TRANSCRIPTION_URL: &str =
    "https://dashscope.aliyuncs.com/api/v1/services/audio/asr/transcription";
const TASK_URL_PREFIX: &str = "https://dashscope.aliyuncs.com/api/v1/tasks";
const DEFAULT_TRANSCRIPTION_MODEL: &str = "fun-asr";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptionParams {
    #[serde(default = "default_transcription_model")]
    pub model: String,
    #[serde(default)]
    pub vocabulary_id: String,
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
            model: default_transcription_model(),
            vocabulary_id: String::new(),
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
            default_transcription_model()
        } else {
            model.to_string()
        }
    }

    fn parameters_value(&self) -> Value {
        let mut parameters = Map::new();
        if !self.vocabulary_id.trim().is_empty() {
            parameters.insert(
                "vocabulary_id".to_string(),
                json!(self.vocabulary_id.trim()),
            );
        }
        let language_hints = self
            .language_hints
            .iter()
            .map(|hint| hint.trim())
            .filter(|hint| !hint.is_empty())
            .collect::<Vec<_>>();
        if !language_hints.is_empty() {
            parameters.insert("language_hints".to_string(), json!(language_hints));
        }
        if let Some(enabled) = self.diarization_enabled {
            parameters.insert("diarization_enabled".to_string(), json!(enabled));
        }
        if let Some(count) = self.speaker_count.filter(|count| *count > 0) {
            parameters.insert("speaker_count".to_string(), json!(count));
        }
        if let Some(channel_id) = &self.channel_id {
            if !channel_id.is_null() {
                parameters.insert("channel_id".to_string(), channel_id.clone());
            }
        }
        if !self.special_word_filter.trim().is_empty() {
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
            return Err(format_task_error(
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

#[derive(Clone, Debug, Serialize)]
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
struct SubmitResponse {
    output: SubmitOutput,
}

#[derive(Debug, Deserialize)]
struct SubmitOutput {
    task_id: String,
}

#[derive(Debug, Deserialize)]
struct TaskResponse {
    output: TaskOutput,
}

#[derive(Debug, Deserialize)]
struct TaskOutput {
    task_status: String,
    #[serde(default)]
    results: Vec<TranscriptionTaskResult>,
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawTranscriptionResult {
    #[serde(default)]
    properties: TranscriptionProperties,
    #[serde(default)]
    transcripts: Vec<TranscriptionTranscript>,
}

#[derive(Debug, Default, Deserialize)]
struct TranscriptionProperties {
    #[serde(default, alias = "originalDurationInMilliseconds")]
    original_duration_in_milliseconds: Option<u64>,
}

pub async fn submit_transcription_task(
    api_key: &str,
    file_url: &str,
    params: &TranscriptionParams,
) -> Result<String, String> {
    if api_key.trim().is_empty() {
        return Err("请先保存阿里云百炼 API Key".to_string());
    }
    if file_url.trim().is_empty() {
        return Err("录音识别文件 URL 不能为空".to_string());
    }

    let model = params.model_id();
    let body = json!({
        "model": model,
        "input": {
            "file_urls": [file_url.trim()],
        },
        "parameters": params.parameters_value(),
    });
    let client = reqwest::Client::new();
    let mut request = client
        .post(TRANSCRIPTION_URL)
        .header("Authorization", format!("Bearer {}", api_key.trim()))
        .header("Content-Type", "application/json")
        .header("X-DashScope-Async", "enable")
        .json(&body);
    if file_url.trim_start().starts_with("oss://") {
        request = request.header("X-DashScope-OssResourceResolve", "enable");
    }
    let resp = request
        .send()
        .await
        .map_err(|e| format!("提交录音识别任务失败：{e}"))?;
    let value = read_json_response(resp, "提交录音识别任务").await?;
    let response: SubmitResponse =
        serde_json::from_value(value).map_err(|e| format!("解析录音识别提交响应失败：{e}"))?;
    if response.output.task_id.trim().is_empty() {
        return Err("提交录音识别任务失败：响应缺少 task_id".to_string());
    }
    Ok(response.output.task_id)
}

pub async fn query_transcription_task(
    api_key: &str,
    task_id: &str,
) -> Result<TranscriptionTaskStatus, String> {
    if task_id.trim().is_empty() {
        return Err("录音识别任务 ID 不能为空".to_string());
    }
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{}/{}", TASK_URL_PREFIX, task_id.trim()))
        .header("Authorization", format!("Bearer {}", api_key.trim()))
        .send()
        .await
        .map_err(|e| format!("查询录音识别任务失败：{e}"))?;
    let value = read_json_response(resp, "查询录音识别任务").await?;
    let response: TaskResponse =
        serde_json::from_value(value).map_err(|e| format!("解析录音识别任务响应失败：{e}"))?;
    Ok(TranscriptionTaskStatus {
        task_status: response.output.task_status,
        result: response.output.results.into_iter().next(),
        code: response.output.code,
        message: response.output.message,
    })
}

pub async fn fetch_transcription_result(url: &str) -> Result<TranscriptionResult, String> {
    if url.trim().is_empty() {
        return Err("录音识别结果地址不能为空".to_string());
    }
    let client = reqwest::Client::new();
    let resp = client
        .get(url.trim())
        .send()
        .await
        .map_err(|e| format!("下载录音识别结果失败：{e}"))?;
    let value = read_json_response(resp, "下载录音识别结果").await?;
    let raw: RawTranscriptionResult =
        serde_json::from_value(value).map_err(|e| format!("解析录音识别结果失败：{e}"))?;
    Ok(TranscriptionResult {
        duration_ms: raw.properties.original_duration_in_milliseconds,
        transcripts: raw.transcripts,
    })
}

fn default_transcription_model() -> String {
    DEFAULT_TRANSCRIPTION_MODEL.to_string()
}

async fn read_json_response(resp: reqwest::Response, action: &str) -> Result<Value, String> {
    let status = resp.status();
    let text = resp
        .text()
        .await
        .map_err(|e| format!("读取{action}响应失败：{e}"))?;
    let value = serde_json::from_str::<Value>(&text)
        .map_err(|e| format!("{action}响应解析失败：{e}（{}）", truncate(&text, 200)))?;
    if !status.is_success() {
        return Err(format!(
            "{action}返回 {status}：{}",
            extract_error_message(&value, &text)
        ));
    }
    Ok(value)
}

fn extract_error_message(value: &Value, text: &str) -> String {
    value
        .get("message")
        .or_else(|| value.get("msg"))
        .or_else(|| value.pointer("/error/message"))
        .or_else(|| value.pointer("/output/message"))
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .unwrap_or_else(|| truncate(text, 200))
}

fn format_task_error(prefix: &str, code: Option<&str>, message: Option<&str>) -> String {
    match (
        code.filter(|v| !v.is_empty()),
        message.filter(|v| !v.is_empty()),
    ) {
        (Some(code), Some(message)) => format!("{prefix} [{code}]：{message}"),
        (Some(code), None) => format!("{prefix} [{code}]"),
        (None, Some(message)) => format!("{prefix}：{message}"),
        (None, None) => prefix.to_string(),
    }
}

fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max_chars).collect();
    out.push('…');
    out
}
