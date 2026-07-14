use crate::prelude::*;
use crate::state::*;

const UNSUPPORTED: &str = "当前平台尚未实现系统音频采集；请改用麦克风输入";

#[tauri::command]
pub(crate) fn start_backend_system_audio(
    _device_name: Option<String>,
    _state: tauri::State<'_, RuntimeState>,
) -> Result<BackendMicStartResponse, String> {
    Err(UNSUPPORTED.into())
}

pub(crate) fn start_backend_system_audio_inner(
    _device_name: Option<String>,
    _state: &RuntimeState,
) -> Result<BackendMicStartResponse, String> {
    Err(UNSUPPORTED.into())
}

pub(crate) fn attach_backend_system_audio_to_asr_inner(
    _session_id: &str,
    _state: &RuntimeState,
) -> Result<BackendMicAttachResponse, String> {
    Err(UNSUPPORTED.into())
}

pub(crate) fn attach_backend_system_audio_raw_inner(
    _state: &RuntimeState,
) -> Result<
    (
        BackendMicAttachResponse,
        tokio::sync::mpsc::UnboundedReceiver<AsrStreamInput>,
    ),
    String,
> {
    Err(UNSUPPORTED.into())
}

pub(crate) fn pause_backend_system_audio_inner(_state: &RuntimeState) -> Result<(), String> {
    Ok(())
}

#[tauri::command]
pub(crate) fn release_backend_system_audio(
    state: tauri::State<'_, RuntimeState>,
) -> Result<(), String> {
    release_backend_system_audio_inner(&state)
}

pub(crate) fn release_backend_system_audio_inner(state: &RuntimeState) -> Result<(), String> {
    let mut guard = state
        .backend_system_audio
        .lock()
        .map_err(|_| "Backend system audio lock failed".to_string())?;
    guard.session_id = None;
    guard.tx = None;
    guard.raw_txs.clear();
    guard.pending.clear();
    guard.buffer.clear();
    guard.last_rms = 0.0;
    Ok(())
}

#[tauri::command]
pub(crate) fn get_backend_system_audio_level(
    _state: tauri::State<'_, RuntimeState>,
) -> Result<f32, String> {
    Ok(0.0)
}
