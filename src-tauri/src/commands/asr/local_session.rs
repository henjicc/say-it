use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::commands::audio::emit_asr_stream_event;
use crate::prelude::*;
use crate::providers::local_asr::{LocalAsrOutput, OfflineVadSession, OnlineSession};
use crate::providers::plugin::LocalModelSpec;
use crate::state::*;

pub(super) async fn start_local_asr_stream(
    app: tauri::AppHandle,
    state: &RuntimeState,
    spec: LocalModelSpec,
    model: String,
    input_sample_rate: u32,
    params: Option<DspParams>,
) -> Result<AsrStreamStartResponse, String> {
    let session_id = Uuid::new_v4().to_string();
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<AsrStreamInput>();
    state
        .asr_streams
        .lock()
        .map_err(|_| "ASR stream lock failed".to_string())?
        .insert(session_id.clone(), AsrStreamHandle { tx });

    let streams = state.asr_streams.clone();
    let task_id = session_id.clone();
    tauri::async_runtime::spawn_blocking(move || {
        run_local_session(
            app,
            task_id,
            streams,
            rx,
            StreamDsp::new(params.unwrap_or_default(), input_sample_rate),
            model,
            spec,
        );
    });
    Ok(AsrStreamStartResponse { session_id })
}

enum Session {
    Online(OnlineSession),
    Offline(OfflineVadSession),
}

fn run_local_session(
    app: tauri::AppHandle,
    session_id: String,
    streams: Arc<Mutex<HashMap<String, AsrStreamHandle>>>,
    mut rx: tokio::sync::mpsc::UnboundedReceiver<AsrStreamInput>,
    mut dsp: StreamDsp,
    model: String,
    spec: LocalModelSpec,
) {
    let session = match spec.engine.as_str() {
        "sherpa-onnx-online" => OnlineSession::create(&spec).map(Session::Online),
        "sherpa-onnx-offline" => OfflineVadSession::create(&spec).map(Session::Offline),
        engine => Err(format!("模型引擎 {engine} 不支持本地 ASR 会话")),
    };
    let mut session = match session {
        Ok(session) => session,
        Err(error) => {
            emit_asr_stream_event(&app, &session_id, "error", json!({ "message": error }));
            cleanup_stream(&streams, &session_id);
            return;
        }
    };
    emit_asr_stream_event(
        &app,
        &session_id,
        "opened",
        json!({
            "message": "Local ASR opened",
            "model": model,
            "pluginId": spec.plugin_id,
            "providerId": spec.provider_id
        }),
    );

    loop {
        match rx.blocking_recv() {
            Some(AsrStreamInput::RawF32(samples)) => {
                let pcm = dsp.process(&samples);
                if pcm.is_empty() {
                    continue;
                }
                let samples = pcm16_to_f32(&pcm);
                let result = match &mut session {
                    Session::Online(session) => Ok(session.accept(&samples)),
                    Session::Offline(session) => {
                        session.accept(&samples).map(|segments| LocalAsrOutput {
                            partial: None,
                            finals: segments.into_iter().map(|item| item.text).collect(),
                        })
                    }
                };
                match result {
                    Ok(output) => emit_output(&app, &session_id, output),
                    Err(error) => {
                        emit_asr_stream_event(
                            &app,
                            &session_id,
                            "error",
                            json!({ "message": error }),
                        );
                        break;
                    }
                }
            }
            Some(AsrStreamInput::Finish) => {
                let result = match session {
                    Session::Online(session) => Ok(session.finish()),
                    Session::Offline(session) => session.finish().map(|segments| LocalAsrOutput {
                        partial: None,
                        finals: segments.into_iter().map(|item| item.text).collect(),
                    }),
                };
                match result {
                    Ok(output) => {
                        emit_output(&app, &session_id, output);
                        emit_asr_stream_event(&app, &session_id, "finish", json!({}));
                    }
                    Err(error) => emit_asr_stream_event(
                        &app,
                        &session_id,
                        "error",
                        json!({ "message": error }),
                    ),
                }
                break;
            }
            Some(AsrStreamInput::Stop) | None => break,
        }
    }
    cleanup_stream(&streams, &session_id);
    emit_asr_stream_event(
        &app,
        &session_id,
        "ended",
        json!({ "message": "Local ASR ended" }),
    );
}

fn emit_output(app: &tauri::AppHandle, session_id: &str, output: LocalAsrOutput) {
    if let Some(text) = output.partial {
        emit_asr_stream_event(
            app,
            session_id,
            "result",
            json!({ "text": text, "final": false }),
        );
    }
    for text in output.finals {
        emit_asr_stream_event(
            app,
            session_id,
            "result",
            json!({ "text": text, "final": true }),
        );
    }
}

fn pcm16_to_f32(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(2)
        .map(|sample| i16::from_le_bytes([sample[0], sample[1]]) as f32 / i16::MAX as f32)
        .collect()
}

fn cleanup_stream(streams: &Arc<Mutex<HashMap<String, AsrStreamHandle>>>, session_id: &str) {
    if let Ok(mut streams) = streams.lock() {
        streams.remove(session_id);
    }
}
