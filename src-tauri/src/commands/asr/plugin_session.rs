use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use crate::commands::audio::emit_asr_stream_event;
use crate::prelude::*;
use crate::providers::plugin::PluginRuntimeSpec;
use crate::providers::plugin_runtime::JsProviderRuntime;
use crate::providers::ProviderProfile;
use crate::state::*;

const FINISH_TIMEOUT: Duration = Duration::from_secs(8);
const SESSION_TIMEOUT: Duration = Duration::from_secs(12 * 60 * 60);

pub(super) async fn start_plugin_asr_stream(
    app: tauri::AppHandle,
    state: &RuntimeState,
    plugin: PluginRuntimeSpec,
    profile: ProviderProfile,
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
        run_plugin_session(
            app,
            task_id,
            streams,
            rx,
            params.map(|params| StreamDsp::new(params, input_sample_rate)),
            model,
            plugin,
            profile,
        );
    });

    Ok(AsrStreamStartResponse { session_id })
}

#[allow(clippy::too_many_arguments)]
fn run_plugin_session(
    app: tauri::AppHandle,
    session_id: String,
    streams: Arc<Mutex<HashMap<String, AsrStreamHandle>>>,
    mut rx: tokio::sync::mpsc::UnboundedReceiver<AsrStreamInput>,
    mut dsp: Option<StreamDsp>,
    model: String,
    plugin: PluginRuntimeSpec,
    profile: ProviderProfile,
) {
    let cancelled = Arc::new(AtomicBool::new(false));
    let runtime = match JsProviderRuntime::create(
        plugin.clone(),
        &profile,
        SESSION_TIMEOUT,
        cancelled,
        HashMap::new(),
    ) {
        Ok(runtime) => runtime,
        Err(error) => {
            emit_asr_stream_event(&app, &session_id, "error", json!({ "message": error }));
            cleanup_stream(&streams, &session_id);
            return;
        }
    };
    if let Err(error) = runtime.call(
        "realtimeStart",
        &json!({
            "providerId": profile.id,
            "model": model,
            "sampleRate": OUTPUT_RATE,
            "config": profile.config,
        }),
        Duration::from_secs(30),
    ) {
        emit_asr_stream_event(&app, &session_id, "error", json!({ "message": error }));
        cleanup_stream(&streams, &session_id);
        return;
    }

    emit_asr_stream_event(
        &app,
        &session_id,
        "opened",
        json!({ "message": "JavaScript plugin opened", "model": model, "pluginId": plugin.plugin_id }),
    );
    flush_events(&runtime, &app, &session_id);
    let mut finishing_at = None;
    let mut stop = false;

    while !stop {
        match rx.try_recv() {
            Ok(AsrStreamInput::RawF32(samples)) => {
                let bytes = dsp
                    .as_mut()
                    .map(|dsp| dsp.process(&samples))
                    .unwrap_or_default();
                if !bytes.is_empty() {
                    if let Err(error) = runtime.call_audio(bytes) {
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
            Ok(AsrStreamInput::Finish) => {
                if let Err(error) = runtime.call("realtimeFinish", &Value::Null, FINISH_TIMEOUT) {
                    emit_asr_stream_event(&app, &session_id, "error", json!({ "message": error }));
                    break;
                }
                finishing_at = Some(Instant::now());
            }
            Ok(AsrStreamInput::Stop) => {
                let _ = runtime.call("realtimeStop", &Value::Null, Duration::from_secs(3));
                stop = true;
            }
            Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => break,
            Err(tokio::sync::mpsc::error::TryRecvError::Empty) => {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        if let Err(error) = runtime.dispatch_host_events() {
            emit_asr_stream_event(&app, &session_id, "error", json!({ "message": error }));
            break;
        }
        if flush_events(&runtime, &app, &session_id) {
            break;
        }
        if finishing_at.is_some_and(|started| started.elapsed() >= FINISH_TIMEOUT) {
            emit_asr_stream_event(
                &app,
                &session_id,
                "finish_timeout",
                json!({ "message": "插件收尾超时" }),
            );
            break;
        }
    }
    cleanup_stream(&streams, &session_id);
    emit_asr_stream_event(
        &app,
        &session_id,
        "ended",
        json!({ "message": "JavaScript plugin ended" }),
    );
}

fn flush_events(runtime: &JsProviderRuntime, app: &tauri::AppHandle, session_id: &str) -> bool {
    runtime
        .take_events()
        .into_iter()
        .any(|event| handle_plugin_event(app, session_id, &event))
}

fn handle_plugin_event(app: &tauri::AppHandle, session_id: &str, value: &Value) -> bool {
    match value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default()
    {
        "ready" => emit_asr_stream_event(
            app,
            session_id,
            "event",
            json!({ "message": "plugin ready" }),
        ),
        "partial" => emit_asr_stream_event(
            app,
            session_id,
            "result",
            json!({ "text": value.get("text").and_then(Value::as_str).unwrap_or_default(), "final": false }),
        ),
        "final" => emit_asr_stream_event(
            app,
            session_id,
            "result",
            json!({ "text": value.get("text").and_then(Value::as_str).unwrap_or_default(), "final": true }),
        ),
        "finished" => {
            emit_asr_stream_event(app, session_id, "finish", json!({}));
            return true;
        }
        "error" => {
            emit_asr_stream_event(
                app,
                session_id,
                "error",
                json!({
                    "code": value.get("code").and_then(Value::as_str).unwrap_or("plugin_error"),
                    "message": value.get("message").and_then(Value::as_str).unwrap_or("插件执行失败")
                }),
            );
            return true;
        }
        "event" => emit_asr_stream_event(app, session_id, "event", value.clone()),
        other => emit_asr_stream_event(
            app,
            session_id,
            "event",
            json!({ "message": "unknown plugin event", "type": other }),
        ),
    }
    false
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
    fn realtime_events_keep_existing_frontend_contract() {
        assert_eq!(FINISH_TIMEOUT, Duration::from_secs(8));
    }
}
