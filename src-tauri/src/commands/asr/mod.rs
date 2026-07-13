mod session;
mod silence_test;

use crate::commands::common::*;
use crate::prelude::*;
use crate::state::*;

use session::AsrSession;

const MODEL_CALL_DEBUG_ENABLED: bool = false;

#[tauri::command]
pub(crate) async fn start_asr_stream(
    app: tauri::AppHandle,
    state: tauri::State<'_, RuntimeState>,
    provider_id: Option<String>,
    model_override: Option<String>,
    sample_rate: Option<u32>,
    params: Option<DspParams>,
) -> Result<AsrStreamStartResponse, String> {
    start_asr_stream_inner(
        app,
        &state,
        provider_id,
        model_override,
        sample_rate,
        params,
    )
    .await
}

pub(crate) async fn start_asr_stream_inner(
    app: tauri::AppHandle,
    state: &RuntimeState,
    provider_id: Option<String>,
    model_override: Option<String>,
    sample_rate: Option<u32>,
    params: Option<DspParams>,
) -> Result<AsrStreamStartResponse, String> {
    let provider_id = resolve_provider_id(&state, "asr", provider_id)?;
    let settings = read_provider_settings(&state)?;
    let profile = find_profile(&settings, &provider_id)
        .cloned()
        .ok_or_else(|| format!("供应商 {provider_id} 不存在"))?;
    let (connector, model) = crate::providers::realtime_connector_for(
        &profile.kind,
        &profile.config,
        model_override.as_deref(),
    )?;
    let req = connector.connect_request()?;
    let (ws_stream, _) = connect_async(req).await.map_err(|e| e.to_string())?;
    let (mut writer, reader) = ws_stream.split();
    for message in connector.start_messages() {
        writer.send(message).await.map_err(|e| e.to_string())?;
    }

    let protocol = AsrSession {
        connector,
        model: model.clone(),
        started: false,
        pending: Vec::new(),
    };
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<AsrStreamInput>();
    let session_id = Uuid::new_v4().to_string();
    if MODEL_CALL_DEBUG_ENABLED {
        eprintln!(
            "[model-call] ON session={} provider={} model={}",
            session_id.get(..8).unwrap_or(&session_id),
            provider_id,
            model
        );
    }

    {
        let mut streams = state
            .asr_streams
            .lock()
            .map_err(|_| "ASR stream lock failed".to_string())?;
        streams.insert(session_id.clone(), AsrStreamHandle { tx: tx.clone() });
    }

    let streams = state.asr_streams.clone();
    let app_handle = app.clone();
    let task_session_id = session_id.clone();
    let stream_sample_rate = sample_rate.unwrap_or(48_000);
    let dsp_info = params.as_ref().map(|p| {
        json!({
            "sample_rate": stream_sample_rate,
            "denoise_enabled": p.denoise_enabled,
            "target_lufs": p.target_lufs,
            "max_gain_db": p.max_gain_db,
            "peak_limit_dbfs": p.peak_limit_dbfs,
            "vad_gate": p.vad_gate,
        })
    });
    let dsp = params.map(|p| StreamDsp::new(p, stream_sample_rate));

    tauri::async_runtime::spawn(session::run_asr_session(
        app_handle,
        task_session_id,
        streams,
        writer,
        reader,
        rx,
        protocol,
        dsp,
        dsp_info,
    ));

    Ok(AsrStreamStartResponse { session_id })
}

#[tauri::command]
pub(crate) fn asr_stream_push_chunk(
    session_id: String,
    audio_base64: String,
    state: tauri::State<'_, RuntimeState>,
) -> Result<(), String> {
    let bytes = STANDARD
        .decode(audio_base64.trim())
        .map_err(|e| format!("invalid base64 audio chunk: {e}"))?;
    if bytes.is_empty() {
        return Ok(());
    }
    let tx = {
        let guard = state
            .asr_streams
            .lock()
            .map_err(|_| "ASR stream lock failed".to_string())?;
        guard
            .get(&session_id)
            .ok_or_else(|| "ASR stream not found".to_string())?
            .tx
            .clone()
    };
    tx.send(AsrStreamInput::Audio(bytes))
        .map_err(|_| "ASR stream channel closed".to_string())
}

#[tauri::command]
pub(crate) fn asr_stream_push_f32_chunk(
    session_id: String,
    audio_base64: String,
    state: tauri::State<'_, RuntimeState>,
) -> Result<(), String> {
    let samples = decode_f32_base64(&audio_base64)?;
    if samples.is_empty() {
        return Ok(());
    }
    let tx = {
        let guard = state
            .asr_streams
            .lock()
            .map_err(|_| "ASR stream lock failed".to_string())?;
        guard
            .get(&session_id)
            .ok_or_else(|| "ASR stream not found".to_string())?
            .tx
            .clone()
    };
    tx.send(AsrStreamInput::RawF32(samples))
        .map_err(|_| "ASR stream channel closed".to_string())
}

#[tauri::command]
pub(crate) fn asr_stream_finish(
    session_id: String,
    state: tauri::State<'_, RuntimeState>,
) -> Result<(), String> {
    let tx = {
        let guard = state
            .asr_streams
            .lock()
            .map_err(|_| "ASR stream lock failed".to_string())?;
        guard
            .get(&session_id)
            .ok_or_else(|| "ASR stream not found".to_string())?
            .tx
            .clone()
    };
    tx.send(AsrStreamInput::Finish)
        .map_err(|_| "ASR stream channel closed".to_string())
}

pub(crate) fn asr_stream_finish_inner(
    session_id: &str,
    state: &RuntimeState,
) -> Result<(), String> {
    let tx = state
        .asr_streams
        .lock()
        .map_err(|_| "ASR stream lock failed".to_string())?
        .get(session_id)
        .ok_or_else(|| "ASR stream not found".to_string())?
        .tx
        .clone();
    tx.send(AsrStreamInput::Finish)
        .map_err(|_| "ASR stream channel closed".to_string())
}

#[tauri::command]
pub(crate) fn stop_asr_stream(
    session_id: String,
    state: tauri::State<'_, RuntimeState>,
) -> Result<(), String> {
    let handle = {
        let mut guard = state
            .asr_streams
            .lock()
            .map_err(|_| "ASR stream lock failed".to_string())?;
        guard.remove(&session_id)
    };
    if let Some(handle) = handle {
        if MODEL_CALL_DEBUG_ENABLED {
            eprintln!(
                "[model-call] OFF requested session={}",
                session_id.get(..8).unwrap_or(&session_id)
            );
        }
        let _ = handle.tx.send(AsrStreamInput::Stop);
    }
    Ok(())
}

pub(crate) fn stop_asr_stream_inner(session_id: &str, state: &RuntimeState) -> Result<(), String> {
    let handle = state
        .asr_streams
        .lock()
        .map_err(|_| "ASR stream lock failed".to_string())?
        .remove(session_id);
    if let Some(handle) = handle {
        let _ = handle.tx.send(AsrStreamInput::Stop);
    }
    Ok(())
}

#[tauri::command]
pub(crate) async fn run_asr_silence_test(
    state: tauri::State<'_, RuntimeState>,
    provider_id: Option<String>,
) -> Result<AsrResponse, String> {
    let provider_id = resolve_provider_id(&state, "asr", provider_id)?;
    let settings = read_provider_settings(&state)?;
    let profile = find_profile(&settings, &provider_id)
        .cloned()
        .ok_or_else(|| format!("供应商 {provider_id} 不存在"))?;
    silence_test::run_realtime_silence_test(&profile).await
}
