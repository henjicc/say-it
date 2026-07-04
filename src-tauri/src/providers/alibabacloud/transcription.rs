use std::path::Path;

use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

const TRANSCRIPTION_URL: &str =
    "https://dashscope.aliyuncs.com/api/v1/services/audio/asr/transcription";
const TASK_URL_PREFIX: &str = "https://dashscope.aliyuncs.com/api/v1/tasks";
const MULTIMODAL_GENERATION_URL: &str =
    "https://dashscope.aliyuncs.com/api/v1/services/aigc/multimodal-generation/generation";
const DEFAULT_TRANSCRIPTION_MODEL: &str = "fun-asr";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TranscriptionModelFamily {
    FunAsr,
    FunAsrFlash,
    Paraformer,
    QwenFlash,
    QwenFiletrans,
}

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

    fn parameters_value(&self, family: TranscriptionModelFamily) -> Value {
        let mut parameters = Map::new();
        if matches!(
            family,
            TranscriptionModelFamily::FunAsr | TranscriptionModelFamily::Paraformer
        ) && !self.vocabulary_id.trim().is_empty()
        {
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
        if matches!(
            family,
            TranscriptionModelFamily::FunAsr | TranscriptionModelFamily::Paraformer
        ) && !language_hints.is_empty() {
            parameters.insert("language_hints".to_string(), json!(language_hints));
        } else if family == TranscriptionModelFamily::QwenFiletrans
            && language_hints.len() == 1
        {
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
    result: Option<TranscriptionTaskResult>,
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
    let family = transcription_model_family(&model);
    let body = json!({
        "model": model,
        "input": transcription_input_value(family, file_url),
        "parameters": params.parameters_value(family),
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

pub async fn recognize_short_audio(
    api_key: &str,
    file_path: &str,
    params: &TranscriptionParams,
) -> Result<TranscriptionResult, String> {
    if api_key.trim().is_empty() {
        return Err("请先保存阿里云百炼 API Key".to_string());
    }
    let model = params.model_id();
    let family = transcription_model_family(&model);
    // 同步短音频接口（multimodal-generation）不支持解析 OSS 临时上传返回的 oss:// 私有资源地址，
    // 必须直接传入公网 URL 或 Base64 Data URI；本地文件走 Data URI。
    let data_uri = build_audio_data_uri(file_path)?;
    let body = match family {
        TranscriptionModelFamily::FunAsrFlash => {
            let format = guess_audio_format(file_path);
            let sample_rate = guess_audio_sample_rate(file_path).to_string();
            json!({
                "model": model,
                "input": {
                    "messages": [
                        {
                            "role": "user",
                            "content": [
                                {
                                    "type": "input_audio",
                                    "input_audio": {
                                        "data": data_uri,
                                    }
                                }
                            ]
                        }
                    ]
                },
                "parameters": {
                    "format": format,
                    "sample_rate": sample_rate,
                }
            })
        }
        TranscriptionModelFamily::QwenFlash => {
            let mut asr_options = json!({
                "enable_itn": false,
            });
            if let Some(language) = params
                .language_hints
                .iter()
                .map(|item| item.trim())
                .find(|item| !item.is_empty())
            {
                asr_options["language"] = json!(language);
            }
            json!({
                "model": model,
                "input": {
                    "messages": [
                        {
                            "role": "system",
                            "content": [{ "text": "" }]
                        },
                        {
                            "role": "user",
                            "content": [{ "audio": data_uri }]
                        }
                    ]
                },
                "parameters": {
                    "asr_options": asr_options,
                }
            })
        }
        other => {
            return Err(format!("模型 {other:?} 不支持同步短音频识别"));
        }
    };

    let client = reqwest::Client::new();
    let resp = client
        .post(MULTIMODAL_GENERATION_URL)
        .header("Authorization", format!("Bearer {}", api_key.trim()))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("提交短音频识别失败：{e}"))?;
    let value = read_json_response(resp, "提交短音频识别").await?;
    parse_short_audio_result(value)
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
    let result = response
        .output
        .result
        .or_else(|| response.output.results.into_iter().next());
    Ok(TranscriptionTaskStatus {
        task_status: response.output.task_status,
        result,
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

pub fn uses_async_transcription_task(model: &str) -> bool {
    matches!(
        transcription_model_family(model),
        TranscriptionModelFamily::FunAsr
            | TranscriptionModelFamily::Paraformer
            | TranscriptionModelFamily::QwenFiletrans
    )
}

fn transcription_model_family(model: &str) -> TranscriptionModelFamily {
    let model = model.trim();
    if model.starts_with("qwen3-asr-flash-filetrans") {
        TranscriptionModelFamily::QwenFiletrans
    } else if model == "qwen3-asr-flash" || model == "qwen3-asr-flash-2026-02-10" {
        TranscriptionModelFamily::QwenFlash
    } else if model == "fun-asr-flash-2026-06-15" {
        TranscriptionModelFamily::FunAsrFlash
    } else if model.starts_with("paraformer") {
        TranscriptionModelFamily::Paraformer
    } else {
        TranscriptionModelFamily::FunAsr
    }
}

fn transcription_input_value(family: TranscriptionModelFamily, file_url: &str) -> Value {
    match family {
        TranscriptionModelFamily::QwenFiletrans => json!({
            "file_url": file_url.trim(),
        }),
        TranscriptionModelFamily::FunAsr | TranscriptionModelFamily::Paraformer => json!({
            "file_urls": [file_url.trim()],
        }),
        TranscriptionModelFamily::FunAsrFlash | TranscriptionModelFamily::QwenFlash => {
            unreachable!("同步短音频模型不应走异步 transcription 接口")
        }
    }
}

fn parse_short_audio_result(value: Value) -> Result<TranscriptionResult, String> {
    let content = value
        .pointer("/output/choices/0/message/content")
        .ok_or_else(|| "短音频识别响应缺少 output.choices[0].message.content".to_string())?;

    let text = match content {
        Value::Array(items) => items
            .iter()
            .filter_map(short_audio_content_text)
            .collect::<Vec<_>>()
            .join(""),
        other => short_audio_content_text(other).unwrap_or_default(),
    }
    .trim()
    .to_string();

    if text.is_empty() {
        return Err("短音频识别成功但响应里没有可用文本".to_string());
    }

    Ok(TranscriptionResult {
        duration_ms: None,
        transcripts: vec![TranscriptionTranscript {
            channel_id: None,
            text,
            sentences: Vec::new(),
        }],
    })
}

fn short_audio_content_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.to_string()),
        Value::Object(map) => map
            .get("text")
            .and_then(Value::as_str)
            .map(ToString::to_string)
            .or_else(|| {
                map.get("input_audio_transcription")
                    .and_then(Value::as_str)
                    .map(ToString::to_string)
            }),
        _ => None,
    }
}

fn build_audio_data_uri(file_path: &str) -> Result<String, String> {
    let bytes = std::fs::read(file_path)
        .map_err(|e| format!("读取待识别音频文件失败：{file_path}（{e}）"))?;
    let mime = guess_audio_mime_type(file_path);
    Ok(format!("data:{mime};base64,{}", STANDARD.encode(bytes)))
}

fn guess_audio_mime_type(file_path: &str) -> &'static str {
    match guess_audio_format(file_path).as_str() {
        "mp3" => "audio/mpeg",
        "m4a" => "audio/mp4",
        "aac" => "audio/aac",
        "flac" => "audio/flac",
        "ogg" => "audio/ogg",
        "opus" => "audio/opus",
        "webm" => "audio/webm",
        "amr" => "audio/amr",
        _ => "audio/wav",
    }
}

fn guess_audio_format(file_path: &str) -> String {
    Path::new(file_path)
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.trim().to_ascii_lowercase())
        .filter(|ext| !ext.is_empty())
        .unwrap_or_else(|| "wav".to_string())
}

fn guess_audio_sample_rate(file_path: &str) -> u32 {
    let _ = file_path;
    16_000
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
