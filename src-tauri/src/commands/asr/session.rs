use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::commands::audio::emit_asr_stream_event;
use crate::prelude::*;
use crate::state::*;

const ASR_FINISH_TIMEOUT: Duration = Duration::from_secs(8);

type WsWriter = futures_util::stream::SplitSink<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    Message,
>;
type WsReader = futures_util::stream::SplitStream<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
>;

pub(super) struct AsrSession {
    pub(super) connector: Box<dyn RealtimeAsrConnector>,
    pub(super) model: String,
    pub(super) started: bool,
    pub(super) pending: Vec<Vec<u8>>,
}

pub(super) async fn run_asr_session(
    app_handle: tauri::AppHandle,
    task_session_id: String,
    streams: Arc<Mutex<HashMap<String, AsrStreamHandle>>>,
    mut writer: WsWriter,
    mut reader: WsReader,
    mut rx: tokio::sync::mpsc::UnboundedReceiver<AsrStreamInput>,
    mut protocol: AsrSession,
    mut dsp: Option<StreamDsp>,
    dsp_info: Option<Value>,
) {
    emit_asr_stream_event(
        &app_handle,
        &task_session_id,
        "opened",
        json!({
            "message": "asr websocket opened",
            "model": &protocol.model,
            "dsp_enabled": dsp.is_some(),
            "dsp": dsp_info,
        }),
    );
    let mut should_exit = false;
    let mut audio_chunks: u64 = 0;
    let mut audio_bytes: u64 = 0;
    let mut finish_sent_at: Option<Instant> = None;

    loop {
        while let Ok(cmd) = rx.try_recv() {
            match cmd {
                AsrStreamInput::Audio(bytes) => {
                    if send_or_queue_audio(
                        &mut writer,
                        &mut protocol,
                        bytes,
                        &app_handle,
                        &task_session_id,
                        &mut audio_chunks,
                        &mut audio_bytes,
                    )
                    .await
                    .is_err()
                    {
                        should_exit = true;
                        break;
                    }
                }
                AsrStreamInput::RawF32(samples) => {
                    let bytes = if let Some(dsp) = dsp.as_mut() {
                        dsp.process(&samples)
                    } else {
                        Vec::new()
                    };
                    if bytes.is_empty() {
                        continue;
                    }
                    if send_or_queue_audio(
                        &mut writer,
                        &mut protocol,
                        bytes,
                        &app_handle,
                        &task_session_id,
                        &mut audio_chunks,
                        &mut audio_bytes,
                    )
                    .await
                    .is_err()
                    {
                        should_exit = true;
                        break;
                    }
                }
                AsrStreamInput::Finish => {
                    let secs = audio_bytes as f64 / (OUTPUT_RATE as f64 * 2.0);
                    dlog!(
                        "[asr {}] 发送 finish（共 {} 块 / {} 字节 ≈ {:.1}s 音频）",
                        task_session_id.get(..8).unwrap_or(&task_session_id),
                        audio_chunks,
                        audio_bytes,
                        secs
                    );
                    if let Err(err) =
                        send_finish(&mut writer, &protocol).await
                    {
                        emit_asr_stream_event(
                            &app_handle,
                            &task_session_id,
                            "error",
                            json!({ "message": format!("发送 finish 失败: {err}"), "stage": "send_finish" }),
                        );
                        should_exit = true;
                        break;
                    }
                    finish_sent_at = Some(Instant::now());
                }
                AsrStreamInput::Stop => {
                    let _ = writer.send(Message::Close(None)).await;
                    should_exit = true;
                    break;
                }
            }
        }
        if should_exit {
            break;
        }
        if let Some(sent_at) = finish_sent_at {
            if sent_at.elapsed() >= ASR_FINISH_TIMEOUT {
                emit_asr_stream_event(
                    &app_handle,
                    &task_session_id,
                    "finish_timeout",
                    json!({
                        "message": "ASR finish timeout; using latest result",
                        "timeout_ms": ASR_FINISH_TIMEOUT.as_millis()
                    }),
                );
                break;
            }
        }

        let message = tokio::time::timeout(Duration::from_millis(50), reader.next()).await;
        let Ok(message) = message else {
            continue;
        };
        let Some(message) = message else {
            break;
        };
        match message {
            Ok(Message::Text(text)) => match protocol.connector.parse_message(&text) {
                AsrEvent::Started => {
                    protocol.started = true;
                    let queued = std::mem::take(&mut protocol.pending);
                    let mut flush_failed = false;
                    for bytes in queued {
                        if let Err(err) = send_audio_message(&mut writer, &protocol, bytes).await {
                            emit_asr_stream_event(
                                &app_handle,
                                &task_session_id,
                                "error",
                                json!({ "message": format!("发送缓冲音频失败: {err}"), "stage": "send_audio" }),
                            );
                            flush_failed = true;
                            break;
                        }
                    }
                    if flush_failed {
                        break;
                    }
                    emit_asr_stream_event(
                        &app_handle,
                        &task_session_id,
                        "event",
                        json!({ "message": "asr task-started", "model": &protocol.model }),
                    );
                }
                AsrEvent::Partial(text) => {
                    emit_asr_stream_event(
                        &app_handle,
                        &task_session_id,
                        "result",
                        json!({ "text": text, "final": false }),
                    );
                }
                AsrEvent::Final(text) => {
                    emit_asr_stream_event(
                        &app_handle,
                        &task_session_id,
                        "result",
                        json!({ "text": text, "final": true }),
                    );
                }
                AsrEvent::TaskFinished => {
                    emit_asr_stream_event(&app_handle, &task_session_id, "finish", json!({}));
                    break;
                }
                AsrEvent::TaskFailed { code, message } => {
                    emit_asr_stream_event(
                        &app_handle,
                        &task_session_id,
                        "error",
                        json!({ "code": code, "message": message }),
                    );
                    break;
                }
                AsrEvent::Other(value) => {
                    emit_asr_stream_event(&app_handle, &task_session_id, "event", value);
                }
            },
            Ok(Message::Close(frame)) => {
                emit_asr_stream_event(
                    &app_handle,
                    &task_session_id,
                    "closed",
                    json!({ "frame": frame.map(|v| format!("{v:?}")) }),
                );
                break;
            }
            Ok(_) => {}
            Err(err) => {
                emit_asr_stream_event(
                    &app_handle,
                    &task_session_id,
                    "error",
                    json!({ "message": err.to_string() }),
                );
                break;
            }
        }
    }
    if let Ok(mut guard) = streams.lock() {
        guard.remove(&task_session_id);
    }
    emit_asr_stream_event(
        &app_handle,
        &task_session_id,
        "ended",
        json!({ "message": "asr stream ended" }),
    );
}

async fn send_or_queue_audio(
    writer: &mut WsWriter,
    protocol: &mut AsrSession,
    bytes: Vec<u8>,
    app: &tauri::AppHandle,
    session_id: &str,
    audio_chunks: &mut u64,
    audio_bytes: &mut u64,
) -> Result<(), ()> {
    if !protocol.started {
        protocol.pending.push(bytes);
        return Ok(());
    }
    let n = bytes.len();
    if let Err(err) = send_audio_message(writer, protocol, bytes).await {
        emit_asr_stream_event(
            app,
            session_id,
            "error",
            json!({ "message": format!("发送音频失败: {err}"), "stage": "send_audio" }),
        );
        return Err(());
    }
    *audio_chunks += 1;
    *audio_bytes += n as u64;
    Ok(())
}

async fn send_audio_message(
    writer: &mut WsWriter,
    protocol: &AsrSession,
    bytes: Vec<u8>,
) -> Result<(), tokio_tungstenite::tungstenite::Error> {
    writer.send(protocol.connector.audio_message(bytes)).await
}

async fn send_finish(
    writer: &mut WsWriter,
    protocol: &AsrSession,
) -> Result<(), tokio_tungstenite::tungstenite::Error> {
    writer.send(protocol.connector.finish_message()).await
}
