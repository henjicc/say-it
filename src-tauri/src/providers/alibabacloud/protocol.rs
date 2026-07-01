use serde::Deserialize;
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::Message;

/// 从 `ProviderProfile.config` 反序列化出的 Fun-ASR 参数。
#[derive(Clone, Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct FunAsrParams {
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub vocabulary_id: String,
    #[serde(default)]
    pub language_hints: Vec<String>,
    #[serde(default)]
    pub semantic_punctuation_enabled: bool,
    #[serde(default = "default_max_sentence_silence")]
    pub max_sentence_silence: u32,
    #[serde(default)]
    pub heartbeat: bool,
    #[serde(default)]
    pub speech_noise_threshold: Option<f64>,
}

fn default_max_sentence_silence() -> u32 {
    1300
}

/// 实时识别使用的模型 ID。创建/更新热词列表时的 target_model 必须与此完全一致，
/// 否则阿里云接口不会报错但热词会静默不生效（见 customization.rs）。
pub const FUN_ASR_MODEL: &str = "fun-asr-realtime";

pub enum FunAsrEvent {
    Started,
    Partial(String),
    Final(String),
    TaskFinished,
    TaskFailed { code: String, message: String },
    Other(Value),
}

/// 复用现有 `StreamDsp` 固定输出的 PCM 16kHz 单声道 16bit 格式，因此 format/sample_rate 不需要可配置。
pub fn build_run_task_message(task_id: &str, params: &FunAsrParams) -> Message {
    let mut parameters = json!({
        "format": "pcm",
        "sample_rate": 16000,
        "max_sentence_silence": params.max_sentence_silence,
    });
    if !params.vocabulary_id.trim().is_empty() {
        parameters["vocabulary_id"] = json!(params.vocabulary_id.trim());
    }
    if !params.language_hints.is_empty() {
        parameters["language_hints"] = json!(params.language_hints);
    }
    if params.semantic_punctuation_enabled {
        parameters["semantic_punctuation_enabled"] = json!(true);
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
            "model": FUN_ASR_MODEL,
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

pub fn parse_server_message(text: &str) -> FunAsrEvent {
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
