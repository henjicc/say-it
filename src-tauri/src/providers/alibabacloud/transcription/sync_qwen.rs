use serde_json::Value;

use super::types::{TranscriptionResult, TranscriptionTranscript};

pub(super) fn parse_short_audio_result(value: Value) -> Result<TranscriptionResult, String> {
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
