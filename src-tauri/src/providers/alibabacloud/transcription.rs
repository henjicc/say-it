use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use super::customization::HotwordEntry;

const TRANSCRIPTION_URL: &str =
    "https://dashscope.aliyuncs.com/api/v1/services/audio/asr/transcription";
const TASK_URL_PREFIX: &str = "https://dashscope.aliyuncs.com/api/v1/tasks";
const MULTIMODAL_GENERATION_URL: &str =
    "https://dashscope.aliyuncs.com/api/v1/services/aigc/multimodal-generation/generation";
const DEFAULT_TRANSCRIPTION_MODEL: &str = "fun-asr-flash-2026-06-15";

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

    fn parameters_value(&self, family: TranscriptionModelFamily, vocabulary_id: &str) -> Value {
        let mut parameters = Map::new();
        if matches!(
            family,
            TranscriptionModelFamily::FunAsr | TranscriptionModelFamily::Paraformer
        ) && !vocabulary_id.trim().is_empty()
        {
            parameters.insert(
                "vocabulary_id".to_string(),
                json!(vocabulary_id.trim()),
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
    vocabulary_id: &str,
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
        "parameters": params.parameters_value(family, vocabulary_id),
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
    hotwords: &[HotwordEntry],
) -> Result<TranscriptionResult, String> {
    if api_key.trim().is_empty() {
        return Err("请先保存阿里云百炼 API Key".to_string());
    }
    let model = params.model_id();
    let family = transcription_model_family(&model);
    // 同步短音频接口（multimodal-generation）不支持解析 OSS 临时上传返回的 oss:// 私有资源地址，
    // 必须直接传入公网 URL 或 Base64 Data URI；本地文件走 Data URI。
    // 请求体大小受限，这里统一先把原始文件转成单声道 16kHz PCM，再按模型分别压缩：
    // fun-asr-flash 的 parameters.format 文档明确支持 opus，直接用 Opus 压到足够小；
    // qwen3-asr-flash 没有 format 字段、全靠 Data URI 的 mediatype 判断格式，只用文档验证过的 mp3。
    let body = match family {
        TranscriptionModelFamily::FunAsrFlash => {
            let data_uri = build_opus_data_uri(file_path)?;
            let mut messages = Vec::new();
            // fun-asr-flash 不支持 vocabulary_id，改用文档里的“上下文增强”：把热词词表拼成一条
            // input_text 消息放在音频消息之前，模型据此提升这些词的识别概率。
            if !hotwords.is_empty() {
                let vocabulary_text = hotwords
                    .iter()
                    .map(|item| item.text.as_str())
                    .collect::<Vec<_>>()
                    .join(" ");
                messages.push(json!({
                    "role": "user",
                    "content": [
                        {
                            "type": "input_text",
                            "text": vocabulary_text,
                        }
                    ]
                }));
            }
            messages.push(json!({
                "role": "user",
                "content": [
                    {
                        "type": "input_audio",
                        "input_audio": {
                            "data": data_uri,
                        }
                    }
                ]
            }));
            json!({
                "model": model,
                "input": {
                    "messages": messages
                },
                "parameters": {
                    "format": "opus",
                    "sample_rate": crate::audio_prep::TARGET_SAMPLE_RATE.to_string(),
                }
            })
        }
        TranscriptionModelFamily::QwenFlash => {
            let data_uri = build_mp3_data_uri(file_path)?;
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
    let mut request = client
        .post(MULTIMODAL_GENERATION_URL)
        .header("Authorization", format!("Bearer {}", api_key.trim()))
        .header("Content-Type", "application/json");
    // fun-asr-flash 的非流式响应只返回“最后一句”的 sentence.words，无法还原多句音频的完整
    // 逐词时间戳（详见音频规格文档的 SSE 累积处理说明）；改用流式模式，累积每个
    // sentence_end=true 事件即可拿到完整的分句/逐词时间戳。qwen3-asr-flash 的响应本身就不含
    // 时间戳字段（无论流式与否都一样），维持非流式即可。
    if family == TranscriptionModelFamily::FunAsrFlash {
        request = request.header("X-DashScope-SSE", "enable");
    }
    let resp = request
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("提交短音频识别失败：{e}"))?;

    if family == TranscriptionModelFamily::FunAsrFlash {
        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| format!("读取短音频识别响应失败：{e}"))?;
        if crate::debug_log_enabled() {
            let short = truncate(&text, 2000);
            crate::dlog!("[recognize_short_audio family={family:?}] response={short}");
        }
        if !status.is_success() {
            return Err(format!(
                "提交短音频识别返回 {status}：{}",
                extract_sse_error_message(&text)
            ));
        }
        return parse_fun_asr_flash_sse(&text);
    }

    let value = read_json_response(resp, "提交短音频识别").await?;
    if crate::debug_log_enabled() {
        let short = truncate(&value.to_string(), 2000);
        crate::dlog!("[recognize_short_audio family={family:?}] response={short}");
    }
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
    crate::providers::registry::uses_async_transcription_task(model)
}

fn transcription_model_family(model: &str) -> TranscriptionModelFamily {
    use crate::providers::registry::{file_transcription_route, FileTranscriptionRoute};

    match file_transcription_route(model) {
        FileTranscriptionRoute::AsyncOss => {
            // AsyncOss 需要进一步区分 FunAsr / Paraformer / QwenFiletrans
            let normalized = model.trim();
            if normalized.starts_with("qwen3-asr-flash-filetrans") {
                TranscriptionModelFamily::QwenFiletrans
            } else if normalized.starts_with("paraformer") {
                TranscriptionModelFamily::Paraformer
            } else {
                TranscriptionModelFamily::FunAsr
            }
        }
        FileTranscriptionRoute::SyncFunAsrFlash => TranscriptionModelFamily::FunAsrFlash,
        FileTranscriptionRoute::SyncQwen => TranscriptionModelFamily::QwenFlash,
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

/// 解析 fun-asr-flash 的 SSE 流式响应。每个事件的 `data:` 行是一个独立 JSON 对象；
/// 当 `output.sentence.sentence_end` 为 true 时该句已定稿，把它累积成一个
/// [`TranscriptionSentence`]（含逐词时间戳）。最后一个事件的 `output.text` 是整段音频的
/// 完整识别文本，直接作为 transcript 的 text。
fn parse_fun_asr_flash_sse(text: &str) -> Result<TranscriptionResult, String> {
    let mut sentences: Vec<TranscriptionSentence> = Vec::new();
    let mut final_text = String::new();
    let mut duration_ms: Option<u64> = None;
    let mut channel_id: Option<Value> = None;
    let mut saw_event = false;

    for line in text.lines() {
        let Some(data) = line.trim().strip_prefix("data:") else {
            continue;
        };
        let data = data.trim();
        if data.is_empty() {
            continue;
        }
        let Ok(event) = serde_json::from_str::<Value>(data) else {
            continue;
        };
        saw_event = true;

        if let Some(text) = event.pointer("/output/text").and_then(Value::as_str) {
            final_text = text.to_string();
        }
        if let Some(seconds) = event.pointer("/usage/duration").and_then(Value::as_u64) {
            duration_ms = Some(seconds.saturating_mul(1000));
        }

        let Some(sentence_val) = event.pointer("/output/sentence") else {
            continue;
        };
        let sentence_end = sentence_val
            .get("sentence_end")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if !sentence_end {
            continue;
        }
        if channel_id.is_none() {
            channel_id = sentence_val.get("channel_id").cloned();
        }
        let sentence: TranscriptionSentence = serde_json::from_value(sentence_val.clone())
            .map_err(|e| format!("解析录音识别句子失败：{e}"))?;
        // fun-asr-flash 真实返回里可能反复把“同一句正在定稿中的整句”重发为 sentence_end=true。
        // 已观察到两种变体：同一个 sentence_id 重发，或 sentence_id 变化但 begin/channel
        // 不变、文本只是向后增长。两种情况都应该覆盖上一条，而不是追加成多条极短重复字幕。
        match sentences.last_mut() {
            Some(last) if should_replace_last_sentence(last, &sentence) => *last = sentence,
            _ => sentences.push(sentence),
        }
    }

    if !saw_event {
        return Err("短音频识别响应为空或格式不正确".to_string());
    }
    if final_text.trim().is_empty() && sentences.is_empty() {
        return Err("短音频识别成功但响应里没有可用文本".to_string());
    }

    Ok(TranscriptionResult {
        duration_ms,
        transcripts: vec![TranscriptionTranscript {
            channel_id,
            text: final_text,
            sentences,
        }],
    })
}

fn should_replace_last_sentence(
    last: &TranscriptionSentence,
    next: &TranscriptionSentence,
) -> bool {
    if last.sentence_id.is_some() && last.sentence_id == next.sentence_id {
        return true;
    }

    if last.begin_time != next.begin_time || last.end_time > next.end_time {
        return false;
    }
    if last.speaker_id != next.speaker_id {
        return false;
    }

    let last_text = last.text.trim();
    let next_text = next.text.trim();
    if last_text.is_empty() || next_text.is_empty() {
        return false;
    }

    next_text == last_text
        || next_text.starts_with(last_text)
        || last_text.starts_with(next_text)
}

fn extract_sse_error_message(text: &str) -> String {
    match serde_json::from_str::<Value>(text) {
        Ok(value) => extract_error_message(&value, text),
        Err(_) => truncate(text, 200),
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

/// 解码任意输入文件、下混单声道并重采样到 16kHz 后，编码为 Ogg-Opus 并打包成 Data URI。
/// 供 fun-asr-flash 使用：其 `parameters.format` 字段文档明确支持 `opus`。
fn build_opus_data_uri(file_path: &str) -> Result<String, String> {
    let mono16k = crate::audio_prep::decode_to_mono_16k(file_path)?;
    let pcm16 = crate::audio_prep::f32_to_i16(&mono16k);
    let opus_bytes = ogg_opus::encode::<{ crate::audio_prep::TARGET_SAMPLE_RATE }, 1>(&pcm16)
        .map_err(|e| format!("编码 Opus 音频失败：{e:?}"))?;
    Ok(format!(
        "data:audio/ogg;base64,{}",
        STANDARD.encode(opus_bytes)
    ))
}

/// 解码任意输入文件、下混单声道并重采样到 16kHz 后，编码为 MP3 并打包成 Data URI。
/// 供 qwen3-asr-flash 使用：该模型没有独立的 format 字段，音频格式全靠 Data URI 的
/// mediatype 判断，文档只验证过 `audio/wav`、`audio/mp3`，因此不能像 fun-asr-flash 一样用 Opus。
fn build_mp3_data_uri(file_path: &str) -> Result<String, String> {
    let mono16k = crate::audio_prep::decode_to_mono_16k(file_path)?;
    let pcm16 = crate::audio_prep::f32_to_i16(&mono16k);
    let mp3_bytes = encode_mp3_mono(&pcm16, crate::audio_prep::TARGET_SAMPLE_RATE)?;
    Ok(format!(
        "data:audio/mpeg;base64,{}",
        STANDARD.encode(mp3_bytes)
    ))
}

fn encode_mp3_mono(pcm16: &[i16], sample_rate: u32) -> Result<Vec<u8>, String> {
    use mp3lame_encoder::{Bitrate, Builder, FlushNoGap, MonoPcm, Quality};

    let builder = Builder::new().ok_or_else(|| "初始化 MP3 编码器失败".to_string())?;
    let builder = builder
        .with_num_channels(1)
        .map_err(|e| format!("设置 MP3 声道数失败：{e:?}"))?;
    let builder = builder
        .with_sample_rate(sample_rate)
        .map_err(|e| format!("设置 MP3 采样率失败：{e:?}"))?;
    let builder = builder
        .with_brate(Bitrate::Kbps64)
        .map_err(|e| format!("设置 MP3 码率失败：{e:?}"))?;
    let builder = builder
        .with_quality(Quality::Best)
        .map_err(|e| format!("设置 MP3 质量失败：{e:?}"))?;
    let mut encoder = builder
        .build()
        .map_err(|e| format!("创建 MP3 编码器失败：{e:?}"))?;

    let input = MonoPcm(pcm16);
    let mut out = Vec::new();
    out.reserve(mp3lame_encoder::max_required_buffer_size(pcm16.len()));
    let n = encoder
        .encode(input, out.spare_capacity_mut())
        .map_err(|e| format!("MP3 编码失败：{e:?}"))?;
    unsafe {
        out.set_len(out.len() + n);
    }
    let n = encoder
        .flush::<FlushNoGap>(out.spare_capacity_mut())
        .map_err(|e| format!("MP3 编码收尾失败：{e:?}"))?;
    unsafe {
        out.set_len(out.len() + n);
    }
    Ok(out)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_prep::write_test_stereo_wav;

    /// 5 秒 44.1kHz 立体声 16-bit PCM 原始体积，用作压缩率的参照基准。
    fn raw_pcm_bytes(seconds: f32, rate: u32) -> usize {
        (rate as f32 * seconds) as usize * 4
    }

    /// 取自文档《Fun-ASR录音文件识别HTTP API参考.md》里 fun-asr-flash 流式响应的原样示例。
    const FUN_ASR_FLASH_SSE_SAMPLE: &str = concat!(
        "id:1\n",
        "event:result\n",
        ":HTTP_STATUS/200\n",
        "data:{\"output\":{\"sentence\":{\"sentence_id\":1,\"sentence_end\":true,\"end_time\":3800,",
        "\"words\":[",
        "{\"end_time\":1040,\"punctuation\":\"\",\"begin_time\":760,\"fixed\":true,\"text\":\"Hello\"},",
        "{\"end_time\":1240,\"punctuation\":\"，\",\"begin_time\":1040,\"fixed\":true,\"text\":\" World\"},",
        "{\"end_time\":1880,\"punctuation\":\"\",\"begin_time\":1360,\"fixed\":true,\"text\":\"这里是\"},",
        "{\"end_time\":2520,\"punctuation\":\"\",\"begin_time\":1880,\"fixed\":true,\"text\":\"阿里巴巴\"},",
        "{\"end_time\":2840,\"punctuation\":\"\",\"begin_time\":2520,\"fixed\":true,\"text\":\"语音\"},",
        "{\"end_time\":3800,\"punctuation\":\"。\",\"begin_time\":2840,\"fixed\":true,\"text\":\"实验室\"}",
        "],\"begin_time\":760,\"text\":\"Hello World，这里是阿里巴巴语音实验室。\",\"channel_id\":0},",
        "\"text\":\"Hello World，这里是阿里巴巴语音实验室。\"},",
        "\"usage\":{\"duration\":4},",
        "\"request_id\":\"fc1582e4-935c-9fc2-a482-a98bf43daa69\"}\n",
        "\n",
    );

    #[test]
    fn parses_fun_asr_flash_sse_sample_from_docs() {
        let result = parse_fun_asr_flash_sse(FUN_ASR_FLASH_SSE_SAMPLE)
            .expect("doc sample should parse");
        assert_eq!(result.duration_ms, Some(4000));
        assert_eq!(result.transcripts.len(), 1);
        let transcript = &result.transcripts[0];
        assert_eq!(transcript.text, "Hello World，这里是阿里巴巴语音实验室。");
        assert_eq!(transcript.channel_id, Some(json!(0)));
        assert_eq!(transcript.sentences.len(), 1);
        let sentence = &transcript.sentences[0];
        assert_eq!(sentence.begin_time, 760);
        assert_eq!(sentence.end_time, 3800);
        assert_eq!(sentence.words.len(), 6);
        assert_eq!(sentence.words[0].text, "Hello");
        assert_eq!(sentence.words[5].text, "实验室");
        assert_eq!(sentence.words[5].punctuation.as_deref(), Some("。"));
    }

    /// 同一个 sentence_id 在稳定过程中反复以 sentence_end=true 重发（每加稳一个词就整句重发一次），
    /// 应只保留每个 sentence_id 的最后一条，而不是把每次重发都当成新句子。
    #[test]
    fn dedups_repeated_sentence_end_events_for_same_sentence_id() {
        let events = concat!(
            "data:{\"output\":{\"sentence\":{\"sentence_id\":1,\"sentence_end\":true,",
            "\"begin_time\":4200,\"end_time\":4500,\"text\":\"那为什么\",",
            "\"words\":[{\"begin_time\":4200,\"end_time\":4500,\"text\":\"那为什么\",\"punctuation\":\"\"}]},",
            "\"text\":\"那为什么\"},\"request_id\":\"r1\"}\n",
            "\n",
            "data:{\"output\":{\"sentence\":{\"sentence_id\":1,\"sentence_end\":true,",
            "\"begin_time\":4200,\"end_time\":5400,\"text\":\"那为什么这些格式转换APP要么一堆广告\",",
            "\"words\":[{\"begin_time\":4200,\"end_time\":5400,\"text\":\"那为什么这些格式转换APP要么一堆广告\",\"punctuation\":\"\"}]},",
            "\"text\":\"那为什么这些格式转换APP要么一堆广告\"},\"request_id\":\"r1\"}\n",
            "\n",
            "data:{\"output\":{\"sentence\":{\"sentence_id\":2,\"sentence_end\":true,",
            "\"begin_time\":5400,\"end_time\":7400,\"text\":\"我就寻思着\",",
            "\"words\":[{\"begin_time\":5400,\"end_time\":7400,\"text\":\"我就寻思着\",\"punctuation\":\"\"}]},",
            "\"text\":\"那为什么这些格式转换APP要么一堆广告 我就寻思着\"},",
            "\"usage\":{\"duration\":7},\"request_id\":\"r1\"}\n",
            "\n",
        );
        let result = parse_fun_asr_flash_sse(events).expect("should parse");
        let transcript = &result.transcripts[0];
        assert_eq!(
            transcript.sentences.len(),
            2,
            "repeated sentence_end for the same sentence_id must collapse into one entry"
        );
        assert_eq!(
            transcript.sentences[0].text,
            "那为什么这些格式转换APP要么一堆广告"
        );
        assert_eq!(transcript.sentences[0].end_time, 5400);
        assert_eq!(transcript.sentences[1].text, "我就寻思着");
    }

    #[test]
    fn dedups_repeated_sentence_end_events_even_if_sentence_id_changes() {
        let events = concat!(
            "data:{\"output\":{\"sentence\":{\"sentence_id\":11,\"sentence_end\":true,",
            "\"channel_id\":0,\"begin_time\":4200,\"end_time\":4500,\"text\":\"那为什么\",",
            "\"words\":[{\"begin_time\":4200,\"end_time\":4500,\"text\":\"那为什么\",\"punctuation\":\"\"}]},",
            "\"text\":\"那为什么\"},\"request_id\":\"r1\"}\n",
            "\n",
            "data:{\"output\":{\"sentence\":{\"sentence_id\":12,\"sentence_end\":true,",
            "\"channel_id\":0,\"begin_time\":4200,\"end_time\":5400,\"text\":\"那为什么这些格式转换APP要么一堆广告\",",
            "\"words\":[{\"begin_time\":4200,\"end_time\":5400,\"text\":\"那为什么这些格式转换APP要么一堆广告\",\"punctuation\":\"\"}]},",
            "\"text\":\"那为什么这些格式转换APP要么一堆广告\"},\"request_id\":\"r1\"}\n",
            "\n",
            "data:{\"output\":{\"sentence\":{\"sentence_id\":13,\"sentence_end\":true,",
            "\"channel_id\":0,\"begin_time\":5400,\"end_time\":7400,\"text\":\"我就寻思着\",",
            "\"words\":[{\"begin_time\":5400,\"end_time\":7400,\"text\":\"我就寻思着\",\"punctuation\":\"\"}]},",
            "\"text\":\"那为什么这些格式转换APP要么一堆广告 我就寻思着\"},",
            "\"usage\":{\"duration\":7},\"request_id\":\"r1\"}\n",
            "\n",
        );
        let result = parse_fun_asr_flash_sse(events).expect("should parse");
        let transcript = &result.transcripts[0];
        assert_eq!(
            transcript.sentences.len(),
            2,
            "adjacent final events with same begin/channel and growing text must collapse"
        );
        assert_eq!(
            transcript.sentences[0].text,
            "那为什么这些格式转换APP要么一堆广告"
        );
        assert_eq!(transcript.sentences[0].end_time, 5400);
        assert_eq!(transcript.sentences[1].text, "我就寻思着");
    }

    #[test]
    fn opus_data_uri_shrinks_and_is_valid() {
        let path = std::env::temp_dir().join("say_it_transcription_opus_test.wav");
        write_test_stereo_wav(&path, 5.0, 44_100);

        let data_uri = build_opus_data_uri(path.to_str().unwrap()).expect("opus encode should succeed");
        assert!(data_uri.starts_with("data:audio/ogg;base64,"));
        let b64 = data_uri.trim_start_matches("data:audio/ogg;base64,");
        let decoded = STANDARD.decode(b64).expect("base64 payload should decode");
        assert!(!decoded.is_empty());
        assert!(
            decoded.len() < raw_pcm_bytes(5.0, 44_100) / 4,
            "opus output ({} bytes) should be much smaller than raw PCM",
            decoded.len()
        );

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn mp3_data_uri_shrinks_and_is_valid() {
        let path = std::env::temp_dir().join("say_it_transcription_mp3_test.wav");
        write_test_stereo_wav(&path, 5.0, 44_100);

        let data_uri = build_mp3_data_uri(path.to_str().unwrap()).expect("mp3 encode should succeed");
        assert!(data_uri.starts_with("data:audio/mpeg;base64,"));
        let b64 = data_uri.trim_start_matches("data:audio/mpeg;base64,");
        let decoded = STANDARD.decode(b64).expect("base64 payload should decode");
        assert!(!decoded.is_empty());
        assert!(
            decoded.len() < raw_pcm_bytes(5.0, 44_100) / 4,
            "mp3 output ({} bytes) should be much smaller than raw PCM",
            decoded.len()
        );

        let _ = std::fs::remove_file(&path);
    }
}
