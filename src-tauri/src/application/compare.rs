//! 模型对比运行时。录音、PCM 扇出、上传文件节奏投喂和子任务收敛均在 Rust 中完成。
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{Emitter, Manager};

use crate::application::audio_session::{AudioLease, AudioOwner};
use crate::application::contract::{
    next_revision, DomainEventEnvelope, DomainRunState, DomainSnapshot,
};
use crate::application::events::BackendEvent;
use crate::audio_dsp::DspParams;
use crate::commands::asr::{
    asr_stream_finish_inner, start_asr_stream_inner, stop_asr_stream_inner,
};
use crate::commands::transcription::transcription_start_inner;
use crate::desktop::backend_mic::{
    attach_backend_mic_raw_inner, pause_backend_mic_inner, release_backend_mic_inner,
    start_backend_mic_inner,
};
use crate::providers::capabilities::TranscriptionParams;
use crate::state::{AsrStreamInput, RuntimeState};

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CompareStartRequest {
    pub(crate) source_mode: String,
    pub(crate) file_path: Option<String>,
    pub(crate) models: Vec<String>,
    pub(crate) device_name: Option<String>,
    pub(crate) params: Option<DspParams>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CompareCellSnapshot {
    pub(crate) index: usize,
    pub(crate) status: String,
    pub(crate) text: String,
    pub(crate) error_message: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CompareSnapshot {
    pub(crate) phase: String,
    pub(crate) cells: Vec<CompareCellSnapshot>,
    pub(crate) playback_progress: Option<PlaybackProgress>,
    pub(crate) error: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlaybackProgress {
    pub(crate) current_ms: u64,
    pub(crate) duration_ms: u64,
}

#[derive(Default)]
pub(crate) struct CompareRuntime {
    inner: Mutex<CompareState>,
    epoch: AtomicU64,
}

#[derive(Default)]
struct CompareState {
    phase: String,
    cells: Vec<CompareCellSnapshot>,
    sessions: HashMap<String, usize>,
    jobs: HashMap<String, usize>,
    models: HashMap<usize, String>,
    raw: Vec<f32>,
    sample_rate: u32,
    recording_drain: Option<tokio::sync::oneshot::Receiver<()>>,
    lease: Option<AudioLease>,
    playback_progress: Option<PlaybackProgress>,
    error: String,
}

impl CompareRuntime {
    fn reset(&self, cells: Vec<CompareCellSnapshot>) -> u64 {
        let epoch = self.epoch.fetch_add(1, Ordering::AcqRel) + 1;
        if let Ok(mut state) = self.inner.lock() {
            *state = CompareState {
                phase: "starting".into(),
                cells,
                ..Default::default()
            };
        }
        epoch
    }
    fn snapshot(&self) -> CompareSnapshot {
        let Ok(state) = self.inner.lock() else {
            return CompareSnapshot {
                phase: "idle".into(),
                cells: vec![],
                playback_progress: None,
                error: "模型对比状态锁失败".into(),
            };
        };
        CompareSnapshot {
            phase: if state.phase.is_empty() {
                "idle".into()
            } else {
                state.phase.clone()
            },
            cells: state.cells.clone(),
            playback_progress: state.playback_progress.clone(),
            error: state.error.clone(),
        }
    }
    fn update_cell(&self, index: usize, status: &str, text: Option<String>, error: Option<String>) {
        if let Ok(mut state) = self.inner.lock() {
            if let Some(cell) = state.cells.iter_mut().find(|cell| cell.index == index) {
                cell.status = status.into();
                if let Some(text) = text {
                    cell.text = text;
                }
                if let Some(error) = error {
                    cell.error_message = error;
                }
            }
        }
    }
    pub(crate) fn domain_snapshot(&self) -> DomainSnapshot {
        let snapshot = self.snapshot();
        DomainSnapshot {
            state: if matches!(
                snapshot.phase.as_str(),
                "recording" | "playing" | "finalizing" | "starting"
            ) {
                DomainRunState::Running
            } else {
                DomainRunState::Idle
            },
            session_id: None,
        }
    }
}

pub(crate) fn initialize(app: tauri::AppHandle) {
    let mut receiver = app.state::<RuntimeState>().backend_events.subscribe();
    tauri::async_runtime::spawn(async move {
        loop {
            match receiver.recv().await {
                Ok(event) => handle_event(&app, event),
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

#[tauri::command]
pub(crate) async fn compare_start(
    app: tauri::AppHandle,
    request: CompareStartRequest,
) -> Result<CompareSnapshot, String> {
    let state = app.state::<RuntimeState>();
    if !matches!(state.compare_runtime.snapshot().phase.as_str(), "" | "idle") {
        return Err("模型对比正在运行".into());
    }
    if request.source_mode == "upload"
        && request
            .file_path
            .as_deref()
            .map(str::trim)
            .unwrap_or_default()
            .is_empty()
    {
        return Err("请先选择音频文件".into());
    }
    let cells = request
        .models
        .iter()
        .enumerate()
        .filter_map(|(index, model)| (!model.trim().is_empty()).then_some((index, model)))
        .map(|(index, _)| CompareCellSnapshot {
            index,
            status: "queued".into(),
            text: String::new(),
            error_message: String::new(),
        })
        .collect::<Vec<_>>();
    if cells.is_empty() {
        return Err("请至少选择一个模型".into());
    }
    let epoch = state.compare_runtime.reset(cells);
    {
        let mut compare = state
            .compare_runtime
            .inner
            .lock()
            .map_err(|_| "模型对比状态锁失败")?;
        compare.models = request
            .models
            .iter()
            .enumerate()
            .filter_map(|(index, model)| {
                (!model.trim().is_empty()).then_some((index, model.clone()))
            })
            .collect();
    }
    let realtime_sample_rate = if request.source_mode == "upload" {
        16_000
    } else {
        48_000
    };
    for (index, model) in request.models.iter().enumerate() {
        if model.trim().is_empty() {
            continue;
        }
        let Some(info) = resolve_model_info(&state, model) else {
            state
                .compare_runtime
                .update_cell(index, "error", None, Some("模型未登记".into()));
            continue;
        };
        if info.category == "realtime" {
            let opened = start_asr_stream_inner(
                app.clone(),
                &state,
                None,
                Some(model.clone()),
                Some(realtime_sample_rate),
                request.params.clone(),
            )
            .await;
            match opened {
                Ok(session) => {
                    state
                        .compare_runtime
                        .inner
                        .lock()
                        .map_err(|_| "模型对比状态锁失败")?
                        .sessions
                        .insert(session.session_id, index);
                    state
                        .compare_runtime
                        .update_cell(index, "connecting", None, None);
                }
                Err(error) => state
                    .compare_runtime
                    .update_cell(index, "error", None, Some(error)),
            }
        }
    }
    if request.source_mode == "record" {
        start_recording(app.clone(), &state, request.device_name, epoch)?;
    } else {
        let path = request
            .file_path
            .filter(|path| !path.trim().is_empty())
            .ok_or("请先选择音频文件")?;
        start_upload(
            app.clone(),
            &state,
            path,
            request.models,
            request.params,
            epoch,
        )
        .await?;
    }
    publish(&app);
    Ok(state.compare_runtime.snapshot())
}

fn start_recording(
    app: tauri::AppHandle,
    state: &RuntimeState,
    device_name: Option<String>,
    epoch: u64,
) -> Result<(), String> {
    let lease = state.audio_session.acquire(AudioOwner::Comparison)?;
    state.audio_session.attach(&lease, "comparison")?;
    let mic = start_backend_mic_inner(device_name, state)?;
    let (_, mut receiver) = attach_backend_mic_raw_inner(state)?;
    let (drain_tx, drain_rx) = tokio::sync::oneshot::channel();
    {
        let mut compare = state
            .compare_runtime
            .inner
            .lock()
            .map_err(|_| "模型对比状态锁失败")?;
        compare.phase = "recording".into();
        compare.sample_rate = mic.sample_rate;
        compare.recording_drain = Some(drain_rx);
        compare.lease = Some(lease);
    }
    tauri::async_runtime::spawn(async move {
        while let Some(AsrStreamInput::RawF32(samples)) = receiver.recv().await {
            let runtime = &app.state::<RuntimeState>().compare_runtime;
            if runtime.epoch.load(Ordering::Acquire) != epoch {
                break;
            }
            let sessions = {
                let mut guard = runtime.inner.lock().ok();
                if let Some(state) = guard.as_mut() {
                    state.raw.extend_from_slice(&samples);
                    state.sessions.keys().cloned().collect::<Vec<_>>()
                } else {
                    vec![]
                }
            };
            for session in sessions {
                if let Some(handle) = app
                    .state::<RuntimeState>()
                    .asr_streams
                    .lock()
                    .ok()
                    .and_then(|streams| streams.get(&session).cloned())
                {
                    let _ = handle.tx.send(AsrStreamInput::RawF32(samples.clone()));
                }
            }
        }
        let _ = drain_tx.send(());
    });
    Ok(())
}

#[tauri::command]
pub(crate) async fn compare_stop(app: tauri::AppHandle) -> Result<CompareSnapshot, String> {
    let state = app.state::<RuntimeState>();
    let snapshot = state.compare_runtime.snapshot();
    if snapshot.phase == "recording" {
        pause_backend_mic_inner(&state)?;
        release_backend_mic_inner(&state)?;
        let recording_drain = state
            .compare_runtime
            .inner
            .lock()
            .map_err(|_| "模型对比状态锁失败")?
            .recording_drain
            .take();
        if let Some(recording_drain) = recording_drain {
            tokio::time::timeout(std::time::Duration::from_secs(2), recording_drain)
                .await
                .map_err(|_| "模型对比尾部音频排空超时".to_string())?
                .map_err(|_| "模型对比尾部音频任务提前结束".to_string())?;
            crate::dlog!("[compare] 尾部音频扇出已排空，开始结束 ASR 会话");
        }
        let (raw, rate, sessions, file_indices) = {
            let mut compare = state
                .compare_runtime
                .inner
                .lock()
                .map_err(|_| "模型对比状态锁失败")?;
            compare.phase = "finalizing".into();
            let raw = std::mem::take(&mut compare.raw);
            let sessions = compare.sessions.keys().cloned().collect::<Vec<_>>();
            let file_indices = compare
                .models
                .iter()
                .filter_map(|(index, model)| {
                    resolve_model_info(&state, model)
                        .filter(|info| info.category == "file")
                        .map(|_| *index)
                })
                .collect::<Vec<_>>();
            (raw, compare.sample_rate, sessions, file_indices)
        };
        for session in sessions {
            let _ = asr_stream_finish_inner(&session, &state);
        }
        if !file_indices.is_empty() {
            if raw.is_empty() {
                for index in file_indices {
                    state.compare_runtime.update_cell(
                        index,
                        "error",
                        None,
                        Some("未录到音频".into()),
                    );
                }
            } else {
                let path = write_wav(&raw, rate)?;
                start_file_jobs(app.clone(), &state, path, file_indices).await;
            }
        }
        release_lease(&state);
        publish(&app);
    }
    Ok(state.compare_runtime.snapshot())
}

#[tauri::command]
pub(crate) fn compare_cancel(app: tauri::AppHandle) -> Result<CompareSnapshot, String> {
    let state = app.state::<RuntimeState>();
    state.compare_runtime.epoch.fetch_add(1, Ordering::AcqRel);
    let (sessions, jobs) = {
        let mut compare = state
            .compare_runtime
            .inner
            .lock()
            .map_err(|_| "模型对比状态锁失败")?;
        compare.phase = "idle".into();
        (
            std::mem::take(&mut compare.sessions),
            std::mem::take(&mut compare.jobs),
        )
    };
    let _ = pause_backend_mic_inner(&state);
    let _ = release_backend_mic_inner(&state);
    for (id, index) in sessions {
        let _ = stop_asr_stream_inner(&id, &state);
        state
            .compare_runtime
            .update_cell(index, "error", None, Some("已取消".into()));
    }
    for (id, index) in jobs {
        let _ = crate::commands::transcription::transcription_cancel_inner(&app, &state, &id);
        state
            .compare_runtime
            .update_cell(index, "error", None, Some("已取消".into()));
    }
    release_lease(&state);
    publish(&app);
    Ok(state.compare_runtime.snapshot())
}

#[tauri::command]
pub(crate) fn get_compare_runtime(state: tauri::State<'_, RuntimeState>) -> CompareSnapshot {
    state.compare_runtime.snapshot()
}

fn resolve_model_info(
    state: &RuntimeState,
    model: &str,
) -> Option<crate::providers::registry::ModelInfo> {
    crate::providers::registry::model_info(model)
        .cloned()
        .or_else(|| {
            state
                .plugin_registry
                .lock()
                .ok()
                .and_then(|plugins| plugins.model(model).cloned())
        })
}

async fn start_upload(
    app: tauri::AppHandle,
    state: &RuntimeState,
    path: String,
    models: Vec<String>,
    params: Option<DspParams>,
    epoch: u64,
) -> Result<(), String> {
    let file_indices = models
        .iter()
        .enumerate()
        .filter(|(_, model)| {
            resolve_model_info(state, model)
                .map(|info| info.category == "file")
                .unwrap_or(false)
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    start_file_jobs(app.clone(), state, path.clone(), file_indices).await;
    let realtime = state
        .compare_runtime
        .inner
        .lock()
        .map_err(|_| "模型对比状态锁失败")?
        .sessions
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    if realtime.is_empty() {
        state
            .compare_runtime
            .inner
            .lock()
            .map_err(|_| "模型对比状态锁失败")?
            .phase = "finalizing".into();
        return Ok(());
    }
    let samples = crate::audio_prep::decode_to_mono_16k(&path)?;
    let total = samples.len();
    {
        let mut compare = state
            .compare_runtime
            .inner
            .lock()
            .map_err(|_| "模型对比状态锁失败")?;
        compare.phase = "playing".into();
        compare.playback_progress = Some(PlaybackProgress {
            current_ms: 0,
            duration_ms: total as u64 * 1000 / 16_000,
        });
    }
    tauri::async_runtime::spawn(async move {
        let chunk = 1600;
        for (offset, part) in samples.chunks(chunk).enumerate() {
            let runtime_state = app.state::<RuntimeState>();
            if runtime_state.compare_runtime.epoch.load(Ordering::Acquire) != epoch {
                return;
            }
            let sessions = runtime_state
                .compare_runtime
                .inner
                .lock()
                .ok()
                .map(|compare| compare.sessions.keys().cloned().collect::<Vec<_>>())
                .unwrap_or_default();
            for id in sessions {
                if let Some(handle) = runtime_state
                    .asr_streams
                    .lock()
                    .ok()
                    .and_then(|streams| streams.get(&id).cloned())
                {
                    let _ = handle.tx.send(AsrStreamInput::RawF32(part.to_vec()));
                }
            }
            if let Ok(mut compare) = runtime_state.compare_runtime.inner.lock() {
                compare.playback_progress = Some(PlaybackProgress {
                    current_ms: ((offset + 1) * chunk).min(total) as u64 * 1000 / 16_000,
                    duration_ms: total as u64 * 1000 / 16_000,
                });
            }
            publish(&app);
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        let state = app.state::<RuntimeState>();
        let sessions = state
            .compare_runtime
            .inner
            .lock()
            .ok()
            .map(|mut compare| {
                compare.phase = "finalizing".into();
                compare.sessions.keys().cloned().collect::<Vec<_>>()
            })
            .unwrap_or_default();
        for id in sessions {
            let _ = asr_stream_finish_inner(&id, &state);
        }
        publish(&app);
    });
    let _ = params;
    Ok(())
}

async fn start_file_jobs(
    app: tauri::AppHandle,
    state: &RuntimeState,
    path: String,
    indices: Vec<usize>,
) {
    for index in indices {
        let model = state
            .compare_runtime
            .inner
            .lock()
            .ok()
            .and_then(|compare| compare.models.get(&index).cloned());
        let Some(model) = model else {
            continue;
        };
        state
            .compare_runtime
            .update_cell(index, "uploading", None, None);
        let params = TranscriptionParams {
            model,
            language_hints: vec![],
            diarization_enabled: None,
            speaker_count: None,
            channel_id: None,
            special_word_filter: String::new(),
        };
        match transcription_start_inner(app.clone(), state, path.clone(), Some(params)).await {
            Ok(job) => {
                if let Ok(mut compare) = state.compare_runtime.inner.lock() {
                    compare.jobs.insert(job.job_id, index);
                }
            }
            Err(error) => state
                .compare_runtime
                .update_cell(index, "error", None, Some(error)),
        }
    }
}

fn write_wav(samples: &[f32], sample_rate: u32) -> Result<String, String> {
    let data_len = (samples.len() * 2) as u32;
    let mut bytes = Vec::with_capacity(44 + data_len as usize);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&sample_rate.to_le_bytes());
    bytes.extend_from_slice(&(sample_rate * 2).to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_len.to_le_bytes());
    for sample in samples {
        bytes
            .extend_from_slice(&((sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16).to_le_bytes());
    }
    let path = std::env::temp_dir().join(format!("say-it-compare-{}.wav", uuid::Uuid::new_v4()));
    std::fs::write(&path, bytes).map_err(|e| format!("写入临时录音文件失败：{e}"))?;
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| "临时文件路径无效".into())
}
fn release_lease(state: &RuntimeState) {
    if let Ok(mut compare) = state.compare_runtime.inner.lock() {
        if let Some(lease) = compare.lease.take() {
            let _ = state.audio_session.release(&lease);
        }
    }
}
fn handle_event(app: &tauri::AppHandle, event: BackendEvent) {
    let state = app.state::<RuntimeState>();
    match event {
        BackendEvent::Asr {
            session_id,
            kind,
            payload,
        } => {
            let index = state
                .compare_runtime
                .inner
                .lock()
                .ok()
                .and_then(|compare| compare.sessions.get(&session_id).copied());
            let Some(index) = index else {
                return;
            };
            if kind == "result" {
                let text = payload
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                state
                    .compare_runtime
                    .update_cell(index, "streaming", Some(text), None);
            } else if kind == "ended" {
                if let Ok(mut compare) = state.compare_runtime.inner.lock() {
                    compare.sessions.remove(&session_id);
                }
                state.compare_runtime.update_cell(index, "done", None, None);
            } else if kind == "error" {
                state.compare_runtime.update_cell(
                    index,
                    "error",
                    None,
                    Some(
                        payload
                            .get("message")
                            .and_then(Value::as_str)
                            .unwrap_or("识别失败")
                            .into(),
                    ),
                );
            }
            settle(&state);
            publish(app);
        }
        BackendEvent::Transcription {
            job_id,
            stage,
            payload,
        } => {
            let index = state
                .compare_runtime
                .inner
                .lock()
                .ok()
                .and_then(|compare| compare.jobs.get(&job_id).copied());
            let Some(index) = index else {
                return;
            };
            match stage.as_str() {
                "uploading" => state
                    .compare_runtime
                    .update_cell(index, "uploading", None, None),
                "submitted" | "polling" => {
                    state
                        .compare_runtime
                        .update_cell(index, "recognizing", None, None)
                }
                "completed" => {
                    let text = payload
                        .pointer("/result/transcripts")
                        .and_then(Value::as_array)
                        .map(|items| {
                            items
                                .iter()
                                .filter_map(|item| item.get("text").and_then(Value::as_str))
                                .collect::<Vec<_>>()
                                .join("\n")
                        })
                        .unwrap_or_default();
                    if let Ok(mut compare) = state.compare_runtime.inner.lock() {
                        compare.jobs.remove(&job_id);
                    }
                    state
                        .compare_runtime
                        .update_cell(index, "done", Some(text), None);
                }
                "error" => {
                    if let Ok(mut compare) = state.compare_runtime.inner.lock() {
                        compare.jobs.remove(&job_id);
                    }
                    state.compare_runtime.update_cell(
                        index,
                        "error",
                        None,
                        Some(
                            payload
                                .get("message")
                                .and_then(Value::as_str)
                                .unwrap_or("识别失败")
                                .into(),
                        ),
                    );
                }
                _ => {}
            }
            settle(&state);
            publish(app);
        }
        _ => {}
    }
}
fn settle(state: &RuntimeState) {
    if let Ok(mut compare) = state.compare_runtime.inner.lock() {
        if compare.phase == "finalizing" && compare.sessions.is_empty() && compare.jobs.is_empty() {
            compare.phase = "idle".into();
        }
    }
}
fn publish(app: &tauri::AppHandle) {
    let state = app.state::<RuntimeState>();
    let revision = next_revision(&state.snapshot_revision);
    let _ = app.emit(
        "domain-event",
        DomainEventEnvelope {
            revision,
            domain: "comparison".into(),
            event_type: "stateChanged".into(),
            session_id: None,
            payload: serde_json::to_value(state.compare_runtime.snapshot())
                .unwrap_or_else(|_| json!({})),
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reset_creates_a_running_snapshot_and_keeps_cell_index() {
        let runtime = CompareRuntime::default();
        runtime.reset(vec![CompareCellSnapshot {
            index: 3,
            status: "queued".into(),
            text: String::new(),
            error_message: String::new(),
        }]);
        assert_eq!(runtime.domain_snapshot().state, DomainRunState::Running);
        runtime.update_cell(3, "done", Some("结果".into()), None);
        let snapshot = runtime.snapshot();
        assert_eq!(snapshot.cells[0].index, 3);
        assert_eq!(snapshot.cells[0].text, "结果");
    }
}
