use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use super::session_wait::{SessionWait, SessionWake};
use crate::commands::audio::emit_asr_stream_event;
use crate::prelude::*;
use crate::providers::plugin::PluginRuntimeSpec;
use crate::providers::plugin_runtime;
use crate::providers::plugin_runtime::JsProviderRuntime;
use crate::providers::{ProviderProfile, RequestCustomization};
use crate::state::*;

const FINISH_TIMEOUT: Duration = Duration::from_secs(8);
const RUNTIME_INITIALIZE_TIMEOUT: Duration = Duration::from_secs(30);

pub(super) async fn start_plugin_asr_stream(
    app: tauri::AppHandle,
    state: &RuntimeState,
    plugin: PluginRuntimeSpec,
    profile: ProviderProfile,
    model: String,
    input_sample_rate: u32,
    params: Option<DspParams>,
) -> Result<super::PreparedAsrStream, String> {
    crate::application::plugin_management::refresh_browser_session_before_runtime(
        &app,
        state,
        &profile.id,
        &plugin,
    )
    .await?;
    let customization = crate::application::customization::resolve_for_model(state, &model);
    let session_id = Uuid::new_v4().to_string();
    let (handle, rx) = AsrStreamHandle::channel();
    state
        .asr_streams
        .lock()
        .map_err(|_| "ASR stream lock failed".to_string())?
        .insert(session_id.clone(), handle);

    let streams = state.asr_streams.clone();
    let task_id = session_id.clone();
    Ok(super::PreparedAsrStream::new(
        session_id,
        streams.clone(),
        move || {
            plugin_runtime::spawn_js_worker("plugin-asr", move || {
                run_plugin_session(
                    app,
                    task_id,
                    streams,
                    rx,
                    super::stream_dsp(params, input_sample_rate),
                    model,
                    plugin,
                    profile,
                    customization,
                );
            })
            .map(|_| ())
        },
    ))
}

#[allow(clippy::too_many_arguments)]
fn run_plugin_session(
    app: tauri::AppHandle,
    session_id: String,
    streams: Arc<Mutex<HashMap<String, AsrStreamHandle>>>,
    mut rx: AsrStreamReceiver,
    mut dsp: StreamDsp,
    model: String,
    plugin: PluginRuntimeSpec,
    profile: ProviderProfile,
    customization: RequestCustomization,
) {
    if rx.is_cancelled() {
        if let Some(error) = rx.take_failure() {
            emit_asr_stream_event(&app, &session_id, "error", json!({ "message": error }));
        }
        cleanup_stream(&streams, &session_id);
        emit_asr_stream_event(
            &app,
            &session_id,
            "ended",
            json!({ "message": "ASR cancelled before initialization" }),
        );
        return;
    }
    let cancelled = rx.cancellation_flag();
    let module_id = match plugin.capability_id(&model, "speech-recognition", true) {
        Ok(value) => value.to_string(),
        Err(error) => {
            let error = rx.take_failure().unwrap_or(error);
            emit_asr_stream_event(&app, &session_id, "error", json!({ "message": error }));
            cleanup_stream(&streams, &session_id);
            emit_asr_stream_event(
                &app,
                &session_id,
                "ended",
                json!({ "message": "ASR initialization failed" }),
            );
            return;
        }
    };
    let runtime = match plugin_runtime::create_plugin_capability_runtime(
        plugin.clone(),
        &profile,
        &session_id,
        RUNTIME_INITIALIZE_TIMEOUT,
        cancelled,
        HashMap::new(),
    ) {
        Ok(runtime) => runtime,
        Err(error) => {
            let error = rx.take_failure().unwrap_or(error);
            emit_asr_stream_event(&app, &session_id, "error", json!({ "message": error }));
            cleanup_stream(&streams, &session_id);
            emit_asr_stream_event(
                &app,
                &session_id,
                "ended",
                json!({ "message": "ASR initialization failed" }),
            );
            return;
        }
    };
    let mut start_payload = serde_json::Map::new();
    start_payload.insert("providerId".into(), json!(profile.id));
    start_payload.insert("model".into(), json!(model));
    start_payload.insert("sampleRate".into(), json!(OUTPUT_RATE));
    start_payload.insert("config".into(), profile.config.clone());
    customization.write_into(&mut start_payload);
    if let Err(error) = runtime.open_capability_session(
        &module_id,
        &Value::Object(start_payload),
        &session_id,
        Duration::from_secs(30),
    ) {
        let error = rx.take_failure().unwrap_or(error);
        emit_asr_stream_event(&app, &session_id, "error", json!({ "message": error }));
        cleanup_stream(&streams, &session_id);
        emit_asr_stream_event(
            &app,
            &session_id,
            "ended",
            json!({ "message": "ASR initialization failed" }),
        );
        return;
    }

    emit_asr_stream_event(
        &app,
        &session_id,
        "opened",
        json!({ "message": "SDK plugin capability opened", "model": model, "pluginId": plugin.plugin_id, "moduleId": module_id }),
    );
    flush_events(&runtime, &app, &session_id);
    let mut finishing_at = None;
    let waiter = SessionWait::new();

    loop {
        match waiter.wait(
            &mut rx,
            runtime.host_events_ready(),
            finishing_at.map(|started| started + FINISH_TIMEOUT),
        ) {
            SessionWake::Input(Some(AsrStreamInput::RawF32(samples))) => {
                let bytes = dsp.process(&samples);
                if !bytes.is_empty() {
                    if let Err(error) = runtime.send_capability_audio(bytes) {
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
            SessionWake::Input(Some(AsrStreamInput::Finish)) => {
                if let Err(error) = runtime.finish_capability_session(FINISH_TIMEOUT) {
                    emit_asr_stream_event(&app, &session_id, "error", json!({ "message": error }));
                    break;
                }
                finishing_at = Some(Instant::now());
            }
            SessionWake::Input(Some(AsrStreamInput::Failed(error))) => {
                let _ = rx.take_failure();
                emit_asr_stream_event(&app, &session_id, "error", json!({ "message": error }));
                break;
            }
            SessionWake::Input(Some(AsrStreamInput::Stop)) => {
                let _ = runtime.close_capability_session();
                break;
            }
            SessionWake::Input(None) => break,
            SessionWake::HostEvents | SessionWake::Deadline => {}
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
    if let Some(error) = rx.take_failure() {
        emit_asr_stream_event(&app, &session_id, "error", json!({ "message": error }));
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
