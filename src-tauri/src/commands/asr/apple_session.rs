use std::collections::HashMap;
use std::process::Stdio;
use std::sync::{Arc, Mutex};

use serde::Deserialize;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

use crate::commands::audio::emit_asr_stream_event;
use crate::prelude::*;
use crate::state::*;

#[derive(Debug, Deserialize)]
struct HelperEvent {
    kind: String,
    #[serde(default)]
    text: String,
    #[serde(default)]
    final_result: bool,
    #[serde(default)]
    message: String,
    #[serde(default)]
    locale: String,
}

pub(super) async fn start_apple_speech_stream(
    app: tauri::AppHandle,
    state: &RuntimeState,
    model: String,
    input_sample_rate: u32,
    params: Option<DspParams>,
) -> Result<AsrStreamStartResponse, String> {
    let capability = crate::providers::apple_speech::status();
    if !capability.available {
        return Err(if capability.message.trim().is_empty() {
            "Apple 本地语音识别需要 macOS 26、受支持的设备和语言".into()
        } else {
            capability.message
        });
    }
    if !capability.installed {
        return Err(format!(
            "Apple 本地语音模型{}尚未安装，请先在“设置 → 密钥与识别”中下载",
            if capability.locale.is_empty() {
                String::new()
            } else {
                format!("（{}）", capability.locale)
            }
        ));
    }

    let session_id = Uuid::new_v4().to_string();
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<AsrStreamInput>();
    state
        .asr_streams
        .lock()
        .map_err(|_| "ASR stream lock failed".to_string())?
        .insert(session_id.clone(), AsrStreamHandle { tx });

    let streams = state.asr_streams.clone();
    let task_id = session_id.clone();
    tauri::async_runtime::spawn(run_apple_session(
        app,
        task_id,
        streams,
        rx,
        StreamDsp::new(params.unwrap_or_default(), input_sample_rate),
        model,
    ));
    Ok(AsrStreamStartResponse { session_id })
}

async fn run_apple_session(
    app: tauri::AppHandle,
    session_id: String,
    streams: Arc<Mutex<HashMap<String, AsrStreamHandle>>>,
    mut rx: tokio::sync::mpsc::UnboundedReceiver<AsrStreamInput>,
    mut dsp: StreamDsp,
    model: String,
) {
    let mut command = crate::providers::apple_speech::command(OUTPUT_RATE);
    let mut child = match command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            emit_asr_stream_event(
                &app,
                &session_id,
                "error",
                json!({ "message": format!("启动 Apple SpeechTranscriber 失败：{error}") }),
            );
            cleanup_stream(&streams, &session_id);
            return;
        }
    };
    let mut stdin = child.stdin.take();
    let Some(stdout) = child.stdout.take() else {
        emit_asr_stream_event(
            &app,
            &session_id,
            "error",
            json!({ "message": "Apple SpeechTranscriber 标准输出不可用" }),
        );
        cleanup_stream(&streams, &session_id);
        return;
    };
    let mut lines = BufReader::new(stdout).lines();
    let mut stderr = child.stderr.take();
    let mut opened = false;
    let mut terminal_event = false;
    let mut stopped = false;

    loop {
        tokio::select! {
            input = rx.recv() => match input {
                Some(AsrStreamInput::RawF32(samples)) => {
                    let pcm = dsp.process(&samples);
                    if pcm.is_empty() { continue; }
                    let bytes = pcm16_as_f32_bytes(&pcm);
                    let Some(writer) = stdin.as_mut() else { continue; };
                    if let Err(error) = writer.write_all(&bytes).await {
                        emit_asr_stream_event(
                            &app,
                            &session_id,
                            "error",
                            json!({ "message": format!("发送音频到 Apple SpeechTranscriber 失败：{error}") }),
                        );
                        terminal_event = true;
                        break;
                    }
                }
                Some(AsrStreamInput::Finish) => {
                    if let Some(mut writer) = stdin.take() {
                        let _ = writer.shutdown().await;
                    }
                }
                Some(AsrStreamInput::Stop) | None => {
                    stopped = true;
                    let _ = child.kill().await;
                    break;
                }
            },
            line = lines.next_line() => match line {
                Ok(Some(line)) => match parse_helper_event(&line) {
                    Ok(event) => match event.kind.as_str() {
                        "opened" => {
                            opened = true;
                            emit_asr_stream_event(
                                &app,
                                &session_id,
                                "opened",
                                json!({
                                    "message": "Apple SpeechAnalyzer opened",
                                    "model": model,
                                    "locale": event.locale,
                                    "onDevice": true
                                }),
                            );
                        }
                        "result" if !event.text.is_empty() => emit_asr_stream_event(
                            &app,
                            &session_id,
                            "result",
                            json!({ "text": event.text, "final": event.final_result }),
                        ),
                        "finish" => {
                            terminal_event = true;
                            emit_asr_stream_event(&app, &session_id, "finish", json!({}));
                            break;
                        }
                        "error" => {
                            terminal_event = true;
                            emit_asr_stream_event(
                                &app,
                                &session_id,
                                "error",
                                json!({ "message": event.message }),
                            );
                            break;
                        }
                        _ => {}
                    },
                    Err(error) => {
                        terminal_event = true;
                        emit_asr_stream_event(
                            &app,
                            &session_id,
                            "error",
                            json!({ "message": error }),
                        );
                        break;
                    }
                },
                Ok(None) => break,
                Err(error) => {
                    terminal_event = true;
                    emit_asr_stream_event(
                        &app,
                        &session_id,
                        "error",
                        json!({ "message": format!("读取 Apple SpeechTranscriber 结果失败：{error}") }),
                    );
                    break;
                }
            }
        }
    }

    stdin.take();
    let exit_status = child.wait().await.ok();
    if !stopped && !terminal_event {
        let mut detail = String::new();
        if let Some(mut stderr) = stderr.take() {
            let _ = stderr.read_to_string(&mut detail).await;
        }
        let message = if !detail.trim().is_empty() {
            detail.trim().to_string()
        } else if !opened {
            "Apple SpeechTranscriber 未能启动".to_string()
        } else {
            format!("Apple SpeechTranscriber 意外结束（{exit_status:?}）")
        };
        emit_asr_stream_event(&app, &session_id, "error", json!({ "message": message }));
    }
    cleanup_stream(&streams, &session_id);
    emit_asr_stream_event(
        &app,
        &session_id,
        "ended",
        json!({ "message": "Apple SpeechAnalyzer ended" }),
    );
}

fn parse_helper_event(line: &str) -> Result<HelperEvent, String> {
    #[derive(Deserialize)]
    struct WireEvent {
        kind: String,
        #[serde(default)]
        text: String,
        #[serde(default, rename = "final")]
        final_result: bool,
        #[serde(default)]
        message: String,
        #[serde(default)]
        locale: String,
    }
    let event: WireEvent = serde_json::from_str(line)
        .map_err(|error| format!("解析 Apple SpeechTranscriber 事件失败：{error}"))?;
    Ok(HelperEvent {
        kind: event.kind,
        text: event.text,
        final_result: event.final_result,
        message: event.message,
        locale: event.locale,
    })
}

fn pcm16_as_f32_bytes(bytes: &[u8]) -> Vec<u8> {
    let mut output = Vec::with_capacity(bytes.len() * 2);
    for sample in bytes.chunks_exact(2) {
        let value = i16::from_le_bytes([sample[0], sample[1]]) as f32 / i16::MAX as f32;
        output.extend_from_slice(&value.to_le_bytes());
    }
    output
}

fn cleanup_stream(streams: &Arc<Mutex<HashMap<String, AsrStreamHandle>>>, session_id: &str) {
    if let Ok(mut streams) = streams.lock() {
        streams.remove(session_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_partial_and_final_events() {
        let partial =
            parse_helper_event(r#"{"kind":"result","text":"你好","final":false}"#).unwrap();
        assert_eq!(partial.text, "你好");
        assert!(!partial.final_result);

        let final_result =
            parse_helper_event(r#"{"kind":"result","text":"你好。","final":true}"#).unwrap();
        assert!(final_result.final_result);
    }

    #[test]
    fn converts_pcm16_to_little_endian_f32() {
        let bytes = pcm16_as_f32_bytes(&[0, 0, 0xff, 0x7f]);
        let values = bytes
            .chunks_exact(4)
            .map(|chunk| f32::from_le_bytes(chunk.try_into().unwrap()))
            .collect::<Vec<_>>();
        assert_eq!(values, vec![0.0, 1.0]);
    }
}
