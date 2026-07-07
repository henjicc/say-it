use crate::prelude::*;
use crate::state::*;

pub(super) async fn run_realtime_silence_test(profile: &ProviderProfile) -> Result<AsrResponse, String> {
    let (connector, model) =
        crate::providers::realtime_connector_for(&profile.kind, &profile.config, None)?;
    if !crate::providers::registry::realtime_family_supports_silence_test(&model) {
        return Err("当前静音测试仅支持 Fun-ASR / Paraformer 实时模型".to_string());
    }
    let req = connector.connect_request()?;
    let (ws_stream, _) = connect_async(req).await.map_err(|e| e.to_string())?;
    let (mut writer, mut reader) = ws_stream.split();
    for message in connector.start_messages() {
        writer.send(message).await.map_err(|e| e.to_string())?;
    }

    let mut events: Vec<Value> = Vec::new();
    let mut partials: Vec<String> = Vec::new();
    let mut final_text = String::new();
    let silence = vec![0_u8; 8192];

    loop {
        let next = tokio::time::timeout(Duration::from_secs(20), reader.next())
            .await
            .map_err(|_| "ASR 等待超时".to_string())?;
        let Some(message) = next else { break };
        let message = message.map_err(|e| e.to_string())?;
        let Message::Text(text) = message else { continue };
        match connector.parse_message(&text) {
            AsrEvent::Started => {
                for chunk in silence.chunks(4096) {
                    writer
                        .send(connector.audio_message(chunk.to_vec()))
                        .await
                        .map_err(|e| e.to_string())?;
                    sleep(Duration::from_millis(40)).await;
                }
                writer
                    .send(connector.finish_message())
                    .await
                    .map_err(|e| e.to_string())?;
            }
            AsrEvent::Partial(text) => {
                if !text.is_empty() {
                    partials.push(text);
                }
            }
            AsrEvent::Final(text) => {
                if !text.is_empty() {
                    final_text = text.clone();
                    partials.push(text);
                }
            }
            AsrEvent::TaskFinished => break,
            AsrEvent::TaskFailed { code, message } => {
                return Err(format!(
                    "{} 上游错误 [{code}]: {message}",
                    profile.display_name
                ));
            }
            other => events.push(asr_event_to_value(other)),
        }
    }

    partials.sort();
    partials.dedup();
    Ok(AsrResponse {
        text: final_text,
        partials,
        events,
    })
}

fn asr_event_to_value(event: AsrEvent) -> Value {
    match event {
        AsrEvent::Started => json!({ "event": "task-started" }),
        AsrEvent::Partial(text) => {
            json!({ "event": "result-generated", "text": text, "final": false })
        }
        AsrEvent::Final(text) => {
            json!({ "event": "result-generated", "text": text, "final": true })
        }
        AsrEvent::TaskFinished => json!({ "event": "task-finished" }),
        AsrEvent::TaskFailed { code, message } => {
            json!({ "event": "task-failed", "code": code, "message": message })
        }
        AsrEvent::Other(value) => value,
    }
}
