use std::io::{self, BufRead, Write};

use serde_json::{json, Value};

fn emit(value: Value) {
    let mut stdout = io::stdout().lock();
    serde_json::to_writer(&mut stdout, &value).expect("serialize protocol event");
    stdout.write_all(b"\n").expect("write protocol newline");
    stdout.flush().expect("flush protocol event");
}

fn invoke(message: &Value) {
    let operation = message.get("operation").and_then(Value::as_str).unwrap_or_default();
    let payload = message.get("payload").cloned().unwrap_or_else(|| json!({}));
    match operation {
        "transcribeFile" => emit(json!({
            "type": "completed",
            "result": { "durationMs": 0, "transcripts": [] }
        })),
        "translate" => emit(json!({
            "type": "completed",
            "result": { "text": payload.get("text").and_then(Value::as_str).unwrap_or_default() }
        })),
        "setHotwords" | "clearHotwords" => {
            emit(json!({ "type": "completed", "result": {} }))
        }
        "getHotwords" => emit(json!({
            "type": "completed", "result": { "hotwords": [] }
        })),
        "action" => emit(json!({
            "type": "completed", "result": { "status": "ok", "message": "diagnostic completed" }
        })),
        _ => emit(json!({
            "type": "error", "code": "unsupported_operation", "message": operation
        })),
    }
}

fn main() {
    for line in io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        let message: Value = match serde_json::from_str(&line) {
            Ok(message) => message,
            Err(error) => {
                emit(json!({ "type": "error", "code": "invalid_json", "message": error.to_string() }));
                break;
            }
        };
        match message.get("type").and_then(Value::as_str).unwrap_or_default() {
            "invoke" => {
                invoke(&message);
                break;
            }
            "start" => emit(json!({ "type": "ready" })),
            "audio" => {
                // Decode pcm16Base64 and forward it to the provider's realtime transport.
            }
            "finish" => {
                emit(json!({ "type": "finished" }));
                break;
            }
            "stop" => break,
            _ => {
                emit(json!({ "type": "error", "code": "unknown_message", "message": "unknown host message" }));
                break;
            }
        }
    }
}
