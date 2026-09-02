use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use crate::commands::audio::emit_asr_stream_event;
use crate::prelude::*;
use crate::providers::sdk_runtime::online::{BuiltinSdkRuntime, BuiltinSdkScope};
use crate::providers::ProviderProfile;
use crate::state::*;

const FINISH_TIMEOUT: Duration = Duration::from_secs(8);

pub(super) async fn start_sdk_stream(
    app: tauri::AppHandle,
    state: &RuntimeState,
    profile: ProviderProfile,
    model: String,
    route: crate::providers::registry::BuiltinSdkAsrRoute,
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
    if let Err(error) =
        crate::providers::plugin_runtime::spawn_js_worker("builtin-asr", move || {
            run_sdk_session(
                app,
                task_id,
                streams,
                rx,
                super::stream_dsp(params, input_sample_rate),
                model,
                route,
                profile,
                credentials,
            );
        })
    {
        cleanup_stream(&state.asr_streams, &session_id);
        return Err(error);
    }
    Ok(AsrStreamStartResponse { session_id })
}

#[allow(clippy::too_many_arguments)]
fn run_sdk_session(
    app: tauri::AppHandle,
    session_id: String,
    streams: Arc<Mutex<HashMap<String, AsrStreamHandle>>>,
    mut rx: tokio::sync::mpsc::UnboundedReceiver<AsrStreamInput>,
    mut dsp: StreamDsp,
    model: String,
    route: crate::providers::registry::BuiltinSdkAsrRoute,
    profile: ProviderProfile,
    credentials: crate::providers::credential_store::CredentialStoreHandle,
) {
    let cancelled = Arc::new(AtomicBool::new(false));
    let scope = match BuiltinSdkScope::speech_recognition(&profile) {
        Ok(scope) => scope,
        Err(error) => {
            emit_asr_stream_event(&app, &session_id, "error", json!({ "message": error }));
            cleanup_stream(&streams, &session_id);
            return;
        }
    };
    let runtime = match BuiltinSdkRuntime::create(
        &profile,
        credentials,
        scope,
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
    if let Err(error) = runtime.realtime_start(
        &route.source,
        &route.module_id,
        realtime_input(&profile, &model),
        &session_id,
    ) {
        emit_asr_stream_event(&app, &session_id, "error", json!({ "message": error }));
        cleanup_stream(&streams, &session_id);
        return;
    }
    emit_asr_stream_event(
        &app,
        &session_id,
        "opened",
        json!({ "message": "AI SDK realtime ASR opened", "model": model, "moduleId": route.module_id }),
    );
    flush_events(&runtime, &app, &session_id);

    let mut finishing_at = None;
    let mut stop = false;
    while !stop {
        match rx.try_recv() {
            Ok(AsrStreamInput::RawF32(samples)) => {
                let bytes = dsp.process(&samples);
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

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct SdkRealtimeAsrInput {
    media_type: &'static str,
    sample_rate_hz: u32,
    channels: u8,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    hints: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    options: Option<SdkBailianRealtimeOptions>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct SdkBailianRealtimeOptions {
    format: &'static str,
    max_sentence_silence_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    vocabulary_id: Option<String>,
    semantic_punctuation_enabled: bool,
    multi_threshold_mode_enabled: bool,
    heartbeat: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    speech_noise_threshold: Option<f64>,
}

fn string_list(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect()
}

fn realtime_input(profile: &ProviderProfile, model: &str) -> Value {
    let mut input = SdkRealtimeAsrInput {
        media_type: "audio/pcm",
        sample_rate_hz: OUTPUT_RATE,
        channels: 1,
        hints: Vec::new(),
        options: None,
    };
    if profile.kind != "sdk:bailian" {
        return serde_json::to_value(input).expect("SDK 实时识别输入只包含可序列化基础类型");
    }
    let config = &profile.config;
    let vocabulary_id = config
        .get("vocabularyIds")
        .and_then(|value| value.get(model))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty());
    input.hints = string_list(config.get("languageHints"));
    input.options = Some(SdkBailianRealtimeOptions {
        format: "pcm",
        max_sentence_silence_ms: config
            .get("maxSentenceSilence")
            .and_then(Value::as_u64)
            .unwrap_or(1300),
        vocabulary_id: vocabulary_id.map(str::to_string),
        semantic_punctuation_enabled: config
            .get("semanticPunctuationEnabled")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        multi_threshold_mode_enabled: config
            .get("multiThresholdModeEnabled")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        heartbeat: config
            .get("heartbeat")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        speech_noise_threshold: config.get("speechNoiseThreshold").and_then(Value::as_f64),
    });
    serde_json::to_value(input).expect("SDK 实时识别输入只包含可序列化基础类型")
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
    fn realtime_routes_come_from_the_shared_model_catalog() {
        let route =
            crate::providers::registry::builtin_sdk_asr_route("seedasr-2.0-realtime").unwrap();
        assert_eq!(route.source, "volcengine-speech-recognition-realtime");
        assert_eq!(
            route.module_id,
            "volcengine.speech-recognition.seedasr-2.0-realtime"
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
        assert!(!input.to_string().contains("null"));
    }

    #[test]
    fn realtime_input_omits_absent_optional_fields() {
        let input = realtime_input(&crate::providers::bailian_profile(), "fun-asr-realtime");
        assert!(input.get("hints").is_none());
        assert!(input["options"].get("vocabularyId").is_none());
        assert!(input["options"].get("speechNoiseThreshold").is_none());
        assert!(!input.to_string().contains("null"));
    }

    #[test]
    fn volcengine_realtime_input_only_contains_supported_audio_contract() {
        let input = realtime_input(
            &crate::providers::volcengine_profile(),
            "seedasr-2.0-realtime",
        );
        assert_eq!(
            input,
            json!({ "mediaType": "audio/pcm", "sampleRateHz": 16_000, "channels": 1 })
        );
    }
}
