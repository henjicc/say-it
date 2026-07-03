use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::Message;

/// 从 `ProviderProfile.config` 反序列化出的实时识别参数。
#[derive(Clone, Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct FunAsrParams {
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub vocabulary_id: String,
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
    HOTWORD_TARGET_MODEL.to_string()
}

/// 创建/更新热词列表时的 target_model 必须与此完全一致，
/// 否则阿里云接口不会报错但热词会静默不生效（见 customization.rs）。
pub const HOTWORD_TARGET_MODEL: &str = "fun-asr-realtime";
pub const FUN_ASR_MODEL: &str = HOTWORD_TARGET_MODEL;

pub enum FunAsrEvent {
    Started,
    Partial(String),
    Final(String),
    TaskFinished,
    TaskFailed { code: String, message: String },
    Other(Value),
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

pub fn realtime_asr_family(model: &str) -> RealtimeAsrFamily {
    if model.trim().starts_with("qwen3-asr-flash-realtime") {
        RealtimeAsrFamily::QwenRealtime
    } else {
        RealtimeAsrFamily::DashscopeDuplex
    }
}

/// 复用现有 `StreamDsp` 固定输出的 PCM 16kHz 单声道 16bit 格式，因此 format/sample_rate 不需要可配置。
pub fn build_run_task_message(task_id: &str, params: &FunAsrParams, model: &str) -> Message {
    let mut parameters = json!({
        "format": "pcm",
        "sample_rate": 16000,
        "max_sentence_silence": params.max_sentence_silence,
    });
    let model = model.trim();
    if (model.starts_with("fun-asr") || model.starts_with("paraformer"))
        && !params.vocabulary_id.trim().is_empty()
    {
        parameters["vocabulary_id"] = json!(params.vocabulary_id.trim());
    }
    if !params.language_hints.is_empty() {
        parameters["language_hints"] = json!(params.language_hints);
    }
    if params.semantic_punctuation_enabled {
        parameters["semantic_punctuation_enabled"] = json!(true);
    } else if params.multi_threshold_mode_enabled {
        parameters["multi_threshold_mode_enabled"] = json!(true);
    }
    if params.heartbeat {
        parameters["heartbeat"] = json!(true);
    }
    if let Some(threshold) = params.speech_noise_threshold {
        parameters["speech_noise_threshold"] = json!(threshold);
    }
    let payload = json!({
        "header": {
            "action": "run-task",
            "task_id": task_id,
            "streaming": "duplex"
        },
        "payload": {
            "task_group": "audio",
            "task": "asr",
            "function": "recognition",
            "model": model,
            "parameters": parameters,
            "input": {}
        }
    });
    Message::Text(payload.to_string().into())
}

pub fn build_finish_task_message(task_id: &str) -> Message {
    let payload = json!({
        "header": {
            "action": "finish-task",
            "task_id": task_id,
            "streaming": "duplex"
        },
        "payload": { "input": {} }
    });
    Message::Text(payload.to_string().into())
}

pub fn build_qwen_session_update_message(params: &FunAsrParams) -> Message {
    let mut session = json!({
        "modalities": ["text"],
        "input_audio_format": "pcm",
        "sample_rate": 16000,
        "turn_detection": {
            "type": "server_vad",
            "threshold": 0.0,
            "silence_duration_ms": params.max_sentence_silence.max(200),
        }
    });
    if let Some(language) = params
        .language_hints
        .iter()
        .map(|item| item.trim())
        .find(|item| !item.is_empty())
    {
        session["input_audio_transcription"] = json!({ "language": language });
    }
    Message::Text(
        json!({
            "type": "session.update",
            "session": session,
        })
        .to_string()
        .into(),
    )
}

pub fn build_qwen_audio_message(bytes: &[u8]) -> Message {
    Message::Text(
        json!({
            "type": "input_audio_buffer.append",
            "audio": STANDARD.encode(bytes),
        })
        .to_string()
        .into(),
    )
}

pub fn build_qwen_finish_message() -> Message {
    Message::Text(json!({ "type": "session.finish" }).to_string().into())
}

pub fn parse_server_message(text: &str, model: &str) -> FunAsrEvent {
    match realtime_asr_family(model) {
        RealtimeAsrFamily::DashscopeDuplex => parse_dashscope_duplex_message(text),
        RealtimeAsrFamily::QwenRealtime => parse_qwen_message(text),
    }
}

fn parse_dashscope_duplex_message(text: &str) -> FunAsrEvent {
    let value: Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(_) => return FunAsrEvent::Other(json!({ "raw": text })),
    };
    let event = value
        .pointer("/header/event")
        .and_then(Value::as_str)
        .unwrap_or("");
    match event {
        "task-started" => FunAsrEvent::Started,
        "result-generated" => {
            let sentence = value.pointer("/payload/output/sentence");
            let is_heartbeat = sentence
                .and_then(|s| s.get("heartbeat"))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if is_heartbeat {
                return FunAsrEvent::Other(value);
            }
            let text = sentence
                .and_then(|s| s.get("text"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let is_final = sentence
                .and_then(|s| s.get("sentence_end"))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if is_final {
                FunAsrEvent::Final(text)
            } else {
                FunAsrEvent::Partial(text)
            }
        }
        "task-finished" => FunAsrEvent::TaskFinished,
        "task-failed" => {
            let code = value
                .pointer("/header/error_code")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let message = value
                .pointer("/header/error_message")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            FunAsrEvent::TaskFailed { code, message }
        }
        _ => FunAsrEvent::Other(value),
    }
}

fn parse_qwen_message(text: &str) -> FunAsrEvent {
    let value: Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(_) => return FunAsrEvent::Other(json!({ "raw": text })),
    };
    let event = value.get("type").and_then(Value::as_str).unwrap_or("");
    match event {
        "session.created" | "session.updated" => FunAsrEvent::Started,
        "conversation.item.input_audio_transcription.text" => {
            let text = value.get("text").and_then(Value::as_str).unwrap_or("");
            let stash = value.get("stash").and_then(Value::as_str).unwrap_or("");
            let merged = format!("{text}{stash}");
            FunAsrEvent::Partial(merged)
        }
        "conversation.item.input_audio_transcription.completed" => {
            let text = value
                .get("transcript")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            FunAsrEvent::Final(text)
        }
        "session.finished" => FunAsrEvent::TaskFinished,
        "error" => {
            let code = value
                .get("code")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let message = value
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            FunAsrEvent::TaskFailed { code, message }
        }
        _ => FunAsrEvent::Other(value),
    }
}
