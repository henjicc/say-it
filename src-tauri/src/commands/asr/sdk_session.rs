use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use crate::commands::audio::emit_asr_stream_event;
use crate::prelude::*;
use crate::providers::sdk_runtime::online::{BuiltinSdkRuntime, BuiltinSdkScope};
use crate::providers::ProviderProfile;
use crate::state::*;

const FINISH_TIMEOUT: Duration = Duration::from_secs(8);

pub(super) async fn start_bailian_sdk_stream(
    app: tauri::AppHandle,
    state: &RuntimeState,
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

    let credentials = state.credentials.clone();
    let streams = state.asr_streams.clone();
    let task_id = session_id.clone();
    tauri::async_runtime::spawn_blocking(move || {
        run_sdk_session(
            app,
            task_id,
            streams,
            rx,
            params.map(|params| StreamDsp::new(params, input_sample_rate)),
            model,
            profile,
            credentials,
        );
    });
    Ok(AsrStreamStartResponse { session_id })
}

#[allow(clippy::too_many_arguments)]
fn run_sdk_session(
    app: tauri::AppHandle,
    session_id: String,
    streams: Arc<Mutex<HashMap<String, AsrStreamHandle>>>,
    mut rx: tokio::sync::mpsc::UnboundedReceiver<AsrStreamInput>,
    mut dsp: Option<StreamDsp>,
    model: String,
    profile: ProviderProfile,
    credentials: crate::providers::credential_store::CredentialStoreHandle,
) {
    let cancelled = Arc::new(AtomicBool::new(false));
    let runtime = match BuiltinSdkRuntime::create(
        &profile,
        credentials,
        BuiltinSdkScope::SpeechRecognition,
        session_id.clone(),
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
    let module_id = format!("bailian.speech-recognition.{model}");
    if let Err(error) =
        runtime.realtime_start(&module_id, realtime_input(&profile, &model), &session_id)
    {
        emit_asr_stream_event(&app, &session_id, "error", json!({ "message": error }));
        cleanup_stream(&streams, &session_id);
        return;
    }
    emit_asr_stream_event(
        &app,
        &session_id,
        "opened",
        json!({ "message": "AI SDK realtime ASR opened", "model": model, "moduleId": module_id }),
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
                    if let Err(error) = runtime.realtime_audio(bytes) {
                        emit_asr_stream_event(
                            &app,
                            &session_id,
                            "error",
                            json!({ "message": error, "stage": "sdk_audio" }),
                        );
                        break;
                    }
                }
            }
            Ok(AsrStreamInput::Finish) => {
                if let Err(error) = runtime.realtime_finish() {
                    emit_asr_stream_event(
                        &app,
                        &session_id,
                        "error",
                        json!({ "message": error, "stage": "sdk_finish" }),
                    );
                    break;
                }
                finishing_at = Some(Instant::now());
            }
            Ok(AsrStreamInput::Stop) => {
                let _ = runtime.realtime_stop();
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
                json!({ "message": "AI SDK 实时识别收尾超时" }),
            );
            break;
        }
    }
    cleanup_stream(&streams, &session_id);
    emit_asr_stream_event(
        &app,
        &session_id,
        "ended",
        json!({ "message": "AI SDK realtime ASR ended" }),
    );
}

fn realtime_input(profile: &ProviderProfile, model: &str) -> Value {
    let config = &profile.config;
    let vocabulary_id = config
        .get("vocabularyIds")
        .and_then(|value| value.get(model))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty());
    json!({
        "mediaType": "audio/pcm",
        "sampleRateHz": OUTPUT_RATE,
        "channels": 1,
        "hints": config.get("languageHints").cloned().unwrap_or_else(|| json!([])),
        "options": {
            "format": "pcm",
            "maxSentenceSilenceMs": config.get("maxSentenceSilence").and_then(Value::as_u64).unwrap_or(1300),
            "vocabularyId": vocabulary_id,
            "semanticPunctuationEnabled": config.get("semanticPunctuationEnabled").and_then(Value::as_bool).unwrap_or(false),
            "multiThresholdModeEnabled": config.get("multiThresholdModeEnabled").and_then(Value::as_bool).unwrap_or(false),
            "heartbeat": config.get("heartbeat").and_then(Value::as_bool).unwrap_or(false),
            "speechNoiseThreshold": config.get("speechNoiseThreshold").and_then(Value::as_f64),
        },
    })
}

fn flush_events(runtime: &BuiltinSdkRuntime, app: &tauri::AppHandle, session_id: &str) -> bool {
    runtime
        .take_events()
        .into_iter()
        .any(|event| handle_sdk_event(app, session_id, &event))
}

fn handle_sdk_event(app: &tauri::AppHandle, session_id: &str, value: &Value) -> bool {
    let event = value.get("event").unwrap_or(value);
    match event
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default()
    {
        "started" => emit_asr_stream_event(
            app,
            session_id,
            "event",
            json!({ "message": "sdk started" }),
        ),
        "partial" => emit_asr_stream_event(
            app,
            session_id,
            "result",
            json!({ "text": event.get("text").and_then(Value::as_str).unwrap_or_default(), "final": false }),
        ),
        "final" => emit_asr_stream_event(
            app,
            session_id,
            "result",
            json!({ "text": event.get("text").and_then(Value::as_str).unwrap_or_default(), "final": true }),
        ),
        "completed" => {
            emit_asr_stream_event(app, session_id, "finish", json!({}));
            return true;
        }
        other => emit_asr_stream_event(
            app,
            session_id,
            "event",
            json!({ "message": "sdk event", "type": other }),
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
    fn realtime_model_maps_to_sdk_module_id() {
        assert_eq!(
            format!("bailian.speech-recognition.{}", "fun-asr-realtime"),
            "bailian.speech-recognition.fun-asr-realtime"
        );
    }

    #[test]
    fn realtime_input_maps_existing_bailian_options_without_secret() {
        let mut profile = crate::providers::bailian_profile();
        profile.config["vocabularyIds"] = json!({ "fun-asr-realtime": "vocab-1" });
        profile.config["languageHints"] = json!(["zh", "en"]);
        let input = realtime_input(&profile, "fun-asr-realtime");
        assert_eq!(input["options"]["vocabularyId"], "vocab-1");
        assert_eq!(input["hints"], json!(["zh", "en"]));
        assert!(input.to_string().find("apiKey").is_none());
    }
}
