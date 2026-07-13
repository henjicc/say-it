use std::io::{self, BufRead, Write};

use serde_json::{json, Value};

fn emit(value: Value) {
    let mut stdout = io::stdout().lock();
    serde_json::to_writer(&mut stdout, &value).expect("serialize protocol event");
    stdout.write_all(b"\n").expect("write protocol newline");
    stdout.flush().expect("flush protocol event");
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
            "start" => emit(json!({ "type": "ready" })),
            "audio" => {
                // Decode pcm16Base64 and forward it to the provider's realtime ASR transport.
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
