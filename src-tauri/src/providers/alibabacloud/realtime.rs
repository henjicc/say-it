use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::{http::Request, Message};
use uuid::Uuid;

use crate::providers::connector::{AsrEvent, RealtimeAsrConnector};
use crate::providers::registry;

use super::protocol::{FunAsrParams, RealtimeAsrFamily};
use super::urls::{qwen_realtime_request, ws_request};

/// DashScope duplex 协议连接器（Fun-ASR / Paraformer 实时模型）。
pub struct DashscopeDuplexConnector {
    api_key: String,
    task_id: String,
    params: FunAsrParams,
    model: String,
}

impl DashscopeDuplexConnector {
    fn new(params: &FunAsrParams, model: &str) -> Self {
        Self {
            api_key: params.api_key.clone(),
            task_id: Uuid::new_v4().to_string(),
            params: params.clone(),
            model: model.to_string(),
        }
    }
}

impl RealtimeAsrConnector for DashscopeDuplexConnector {
    fn connect_request(&self) -> Result<Request<()>, String> {
        ws_request(&self.api_key)
    }

    fn start_messages(&self) -> Vec<Message> {
        vec![build_run_task_message(&self.task_id, &self.params, &self.model)]
    }

    fn audio_message(&self, bytes: Vec<u8>) -> Message {
        Message::Binary(bytes.into())
    }

    fn finish_message(&self) -> Message {
        build_finish_task_message(&self.task_id)
    }

    fn parse_message(&self, text: &str) -> AsrEvent {
        parse_dashscope_duplex_message(text)
    }
}

/// Qwen realtime 协议连接器。
pub struct QwenRealtimeConnector {
    api_key: String,
    params: FunAsrParams,
    model: String,
}

impl QwenRealtimeConnector {
    fn new(params: &FunAsrParams, model: &str) -> Self {
        Self {
            api_key: params.api_key.clone(),
            params: params.clone(),
            model: model.to_string(),
        }
    }
}

impl RealtimeAsrConnector for QwenRealtimeConnector {
    fn connect_request(&self) -> Result<Request<()>, String> {
        qwen_realtime_request(&self.api_key, &self.model)
    }

    fn start_messages(&self) -> Vec<Message> {
        vec![build_qwen_session_update_message(&self.params)]
    }

    fn audio_message(&self, bytes: Vec<u8>) -> Message {
        build_qwen_audio_message(&bytes)
    }

    fn finish_message(&self) -> Message {
        build_qwen_finish_message()
    }

    fn parse_message(&self, text: &str) -> AsrEvent {
        parse_qwen_message(text)
    }
}

/// 按模型的协议族选取对应连接器实现，是新增协议族时唯一要接线的地方。
pub fn realtime_connector(params: &FunAsrParams, model: &str) -> Box<dyn RealtimeAsrConnector> {
    match registry::realtime_asr_family(model) {
        RealtimeAsrFamily::DashscopeDuplex => Box::new(DashscopeDuplexConnector::new(params, model)),
        RealtimeAsrFamily::QwenRealtime => Box::new(QwenRealtimeConnector::new(params, model)),
    }
}

// ------- 以下消息构造/解析逻辑从原 protocol.rs 逐字段搬运，不改任何字段与判断 -------

/// 复用现有 `StreamDsp` 固定输出的 PCM 16kHz 单声道 16bit 格式，因此 format/sample_rate 不需要可配置。
fn build_run_task_message(task_id: &str, params: &FunAsrParams, model: &str) -> Message {
    let mut parameters = json!({
        "format": "pcm",
        "sample_rate": 16000,
        "max_sentence_silence": params.max_sentence_silence,
    });
    let model = model.trim();
    let vocabulary_id = params.vocabulary_ids.get(model).map(String::as_str).unwrap_or("");
    if registry::supports_vocabulary(model) && !vocabulary_id.trim().is_empty() {
        parameters["vocabulary_id"] = json!(vocabulary_id.trim());
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

fn build_finish_task_message(task_id: &str) -> Message {
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

fn build_qwen_session_update_message(params: &FunAsrParams) -> Message {
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

fn build_qwen_audio_message(bytes: &[u8]) -> Message {
    Message::Text(
        json!({
            "type": "input_audio_buffer.append",
            "audio": STANDARD.encode(bytes),
        })
        .to_string()
        .into(),
    )
}

fn build_qwen_finish_message() -> Message {
    Message::Text(json!({ "type": "session.finish" }).to_string().into())
}

fn parse_dashscope_duplex_message(text: &str) -> AsrEvent {
    let value: Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(_) => return AsrEvent::Other(json!({ "raw": text })),
    };
    let event = value
        .pointer("/header/event")
        .and_then(Value::as_str)
        .unwrap_or("");
    match event {
        "task-started" => AsrEvent::Started,
        "result-generated" => {
            let sentence = value.pointer("/payload/output/sentence");
            let is_heartbeat = sentence
                .and_then(|s| s.get("heartbeat"))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if is_heartbeat {
                return AsrEvent::Other(value);
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
                AsrEvent::Final(text)
            } else {
                AsrEvent::Partial(text)
            }
        }
        "task-finished" => AsrEvent::TaskFinished,
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
            AsrEvent::TaskFailed { code, message }
        }
        _ => AsrEvent::Other(value),
    }
}

fn parse_qwen_message(text: &str) -> AsrEvent {
    let value: Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(_) => return AsrEvent::Other(json!({ "raw": text })),
    };
    let event = value.get("type").and_then(Value::as_str).unwrap_or("");
    match event {
        "session.created" | "session.updated" => AsrEvent::Started,
        "conversation.item.input_audio_transcription.text" => {
            let text = value.get("text").and_then(Value::as_str).unwrap_or("");
            let stash = value.get("stash").and_then(Value::as_str).unwrap_or("");
            let merged = format!("{text}{stash}");
            AsrEvent::Partial(merged)
        }
        "conversation.item.input_audio_transcription.completed" => {
            let text = value
                .get("transcript")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            AsrEvent::Final(text)
        }
        "session.finished" => AsrEvent::TaskFinished,
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
            AsrEvent::TaskFailed { code, message }
        }
        _ => AsrEvent::Other(value),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_params() -> FunAsrParams {
        FunAsrParams {
            api_key: "test-key".to_string(),
            model: "fun-asr-realtime".to_string(),
            max_sentence_silence: 1300,
            ..Default::default()
        }
    }

    #[test]
    fn dashscope_connector_builds_run_task_message_matching_original_shape() {
        let params = base_params();
        let connector = DashscopeDuplexConnector::new(&params, "fun-asr-realtime");
        let messages = connector.start_messages();
        assert_eq!(messages.len(), 1);
        let Message::Text(text) = &messages[0] else {
            panic!("expected text message");
        };
        let value: Value = serde_json::from_str(text).unwrap();
        assert_eq!(value["header"]["action"], "run-task");
        assert_eq!(value["header"]["streaming"], "duplex");
        assert_eq!(value["payload"]["model"], "fun-asr-realtime");
        assert_eq!(value["payload"]["parameters"]["max_sentence_silence"], 1300);
    }

    #[test]
    fn dashscope_connector_audio_message_is_binary() {
        let params = base_params();
        let connector = DashscopeDuplexConnector::new(&params, "fun-asr-realtime");
        let message = connector.audio_message(vec![1, 2, 3]);
        assert!(matches!(message, Message::Binary(_)));
    }

    #[test]
    fn dashscope_connector_finish_message_reuses_task_id_from_start() {
        let params = base_params();
        let connector = DashscopeDuplexConnector::new(&params, "fun-asr-realtime");
        let Message::Text(start_text) = &connector.start_messages()[0] else {
            panic!("expected text message");
        };
        let start_value: Value = serde_json::from_str(start_text).unwrap();
        let task_id = start_value["header"]["task_id"].as_str().unwrap().to_string();

        let Message::Text(finish_text) = connector.finish_message() else {
            panic!("expected text message");
        };
        let finish_value: Value = serde_json::from_str(&finish_text).unwrap();
        assert_eq!(finish_value["header"]["action"], "finish-task");
        assert_eq!(finish_value["header"]["task_id"], task_id);
    }

    #[test]
    fn dashscope_connector_parses_task_started_and_result_events() {
        let params = base_params();
        let connector = DashscopeDuplexConnector::new(&params, "fun-asr-realtime");

        let started = connector.parse_message(r#"{"header":{"event":"task-started"}}"#);
        assert!(matches!(started, AsrEvent::Started));

        let partial = connector.parse_message(
            r#"{"header":{"event":"result-generated"},"payload":{"output":{"sentence":{"text":"你好","sentence_end":false}}}}"#,
        );
        match partial {
            AsrEvent::Partial(text) => assert_eq!(text, "你好"),
            other => panic!("expected Partial, got {other:?}"),
        }

        let final_event = connector.parse_message(
            r#"{"header":{"event":"result-generated"},"payload":{"output":{"sentence":{"text":"你好","sentence_end":true}}}}"#,
        );
        match final_event {
            AsrEvent::Final(text) => assert_eq!(text, "你好"),
            other => panic!("expected Final, got {other:?}"),
        }
    }

    #[test]
    fn qwen_connector_builds_session_update_message() {
        let mut params = base_params();
        params.model = "qwen3-asr-flash-realtime".to_string();
        params.language_hints = vec!["zh".to_string()];
        let connector = QwenRealtimeConnector::new(&params, "qwen3-asr-flash-realtime");
        let messages = connector.start_messages();
        assert_eq!(messages.len(), 1);
        let Message::Text(text) = &messages[0] else {
            panic!("expected text message");
        };
        let value: Value = serde_json::from_str(text).unwrap();
        assert_eq!(value["type"], "session.update");
        assert_eq!(value["session"]["input_audio_transcription"]["language"], "zh");
    }

    #[test]
    fn qwen_connector_finish_message_is_session_finish() {
        let mut params = base_params();
        params.model = "qwen3-asr-flash-realtime".to_string();
        let connector = QwenRealtimeConnector::new(&params, "qwen3-asr-flash-realtime");
        let Message::Text(text) = connector.finish_message() else {
            panic!("expected text message");
        };
        let value: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["type"], "session.finish");
    }

    #[test]
    fn qwen_connector_parses_transcription_events() {
        let mut params = base_params();
        params.model = "qwen3-asr-flash-realtime".to_string();
        let connector = QwenRealtimeConnector::new(&params, "qwen3-asr-flash-realtime");

        let started = connector.parse_message(r#"{"type":"session.created"}"#);
        assert!(matches!(started, AsrEvent::Started));

        let partial = connector
            .parse_message(r#"{"type":"conversation.item.input_audio_transcription.text","text":"你","stash":"好"}"#);
        match partial {
            AsrEvent::Partial(text) => assert_eq!(text, "你好"),
            other => panic!("expected Partial, got {other:?}"),
        }

        let final_event = connector.parse_message(
            r#"{"type":"conversation.item.input_audio_transcription.completed","transcript":"你好"}"#,
        );
        match final_event {
            AsrEvent::Final(text) => assert_eq!(text, "你好"),
            other => panic!("expected Final, got {other:?}"),
        }
    }

    #[test]
    fn realtime_connector_factory_picks_family_by_registry() {
        let params = base_params();
        let duplex = realtime_connector(&params, "fun-asr-realtime");
        assert!(matches!(duplex.finish_message(), Message::Text(_)));

        let qwen = realtime_connector(&params, "qwen3-asr-flash-realtime");
        let Message::Text(text) = qwen.finish_message() else {
            panic!("expected text message");
        };
        assert_eq!(text, r#"{"type":"session.finish"}"#);
    }
}
