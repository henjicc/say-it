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

#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CompareCellSnapshot {
    pub(crate) index: usize,
    pub(crate) status: String,
    pub(crate) text: String,
    pub(crate) error_message: String,
    /// 已经收到 `final` 的句子，仅用于服务端累计，不投影给前端。
    #[serde(skip)]
    pub(crate) committed: String,
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
    /// 实时识别 `result` 事件里的 `text` 是**当前这一句**，不是整段累计文本。
    ///
    /// 因此不能像 `update_cell` 那样整体替换：收到 `final` 必须把这一句落到
    /// `committed` 上，否则下一句的第一个 partial 就会把上一句冲掉，多句录音
    /// 最终只剩最后一句。语义与 `dictation.rs` 的 `commit_current_segment`
    /// 和字幕侧的 `document.commit` 一致。
    fn update_streaming(&self, index: usize, segment: &str, is_final: bool) {
        if let Ok(mut state) = self.inner.lock() {
            if let Some(cell) = state.cells.iter_mut().find(|cell| cell.index == index) {
                cell.status = "streaming".into();
                cell.text = format!("{}{segment}", cell.committed);
                if is_final {
                    cell.committed = cell.text.clone();
                }
            }
        }
    }
    /// 收回启动失败的现场：phase 回到 idle、未完成的格子标成错误，并把需要在外部
    /// 关闭的 ASR 会话与音频租约交出去。
    fn abort(&self, error: &str) -> (HashMap<String, usize>, Option<AudioLease>) {
        let Ok(mut state) = self.inner.lock() else {
            return (HashMap::new(), None);
        };
        state.phase = "idle".into();
        state.error = error.to_string();
        state.recording_drain = None;
        state.playback_progress = None;
        for cell in &mut state.cells {
            if !matches!(cell.status.as_str(), "done" | "error") {
                cell.status = "error".into();
                cell.error_message = error.to_string();
            }
        }
        (std::mem::take(&mut state.sessions), state.lease.take())
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
            ..Default::default()
        })
        .collect::<Vec<_>>();
    if cells.is_empty() {
        return Err("请至少选择一个模型".into());
    }
    let epoch = state.compare_runtime.reset(cells);
    if let Err(error) = start_all(&app, &state, request, epoch).await {
        return Err(abort_start(&app, &state, error));
    }
    publish(&app);
    Ok(state.compare_runtime.snapshot())
}

/// 启动流程中途失败时，把已经建立起来的东西全部收回。
///
/// 此前 `compare_start` 用裸 `?` 直接返回：已经建好的实时 ASR 会话不会被回收——每个各占
/// 一个 worker 线程（SDK / 插件是 `try_recv` + sleep 无限轮询，本地 sherpa 是
/// `blocking_recv` 永久阻塞，把 200MB+ 权重钉在内存），而 `phase` 永远停在 `starting`，
/// 用户此后再点「开始对比」只会得到「模型对比正在运行」，只能重启应用。
fn abort_start(app: &tauri::AppHandle, state: &RuntimeState, error: String) -> String {
    let (sessions, lease) = state.compare_runtime.abort(&error);
    let _ = release_backend_mic_inner(state);
    for (session_id, _) in sessions {
        let _ = stop_asr_stream_inner(&session_id, state);
    }
    if let Some(lease) = lease {
        let _ = state.audio_session.release(&lease);
    }
    publish(app);
    error
}

async fn start_all(
    app: &tauri::AppHandle,
    state: &RuntimeState,
    request: CompareStartRequest,
    epoch: u64,
) -> Result<(), String> {
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
    // 录音模式的真实采样率由麦克风决定（44.1k 与 48k 都常见），必须先把麦克风拉起来
    // 拿到实际值再开实时流。此前这里硬编码 48k：44.1k 设备上送去识别的 PCM 会被按
    // 48k 解读，等于整段音频加速 8.8%，识别质量明显下降。与 `dictation.rs` 先
    // `start_backend_mic_inner` 记录 `sample_rate`、再 `open_asr` 的顺序保持一致。
    let realtime_sample_rate = if request.source_mode == "upload" {
        16_000
    } else {
        start_recording(app.clone(), &state, request.device_name, epoch)?
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
                    // 录音模式下麦克风在建流之前就开始采集了，握手期间的样本只进了
                    // `raw`。在同一把锁里登记会话并取走已采集的样本补发给新流，既不会
                    // 丢开头，也不会和推流循环重复发送。
                    let backlog = {
                        let mut compare = state
                            .compare_runtime
                            .inner
                            .lock()
                            .map_err(|_| "模型对比状态锁失败")?;
                        compare.sessions.insert(session.session_id.clone(), index);
                        compare.raw.clone()
                    };
                    if !backlog.is_empty() {
                        if let Some(handle) = state
                            .asr_streams
                            .lock()
                            .ok()
                            .and_then(|streams| streams.get(&session.session_id).cloned())
                        {
                            let _ = handle.tx.send(AsrStreamInput::RawF32(backlog));
                        }
                    }
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
    if request.source_mode != "record" {
        let path = request
            .file_path
            .filter(|path| !path.trim().is_empty())
            .ok_or("请先选择音频文件")?;
        start_upload(
            app.clone(),
            state,
            path,
            request.models,
            request.params,
            epoch,
        )
        .await?;
    }
    Ok(())
}

/// 拉起麦克风并开始把 PCM 扇出给各子任务，返回**麦克风的实际采样率**。
fn start_recording(
    app: tauri::AppHandle,
    state: &RuntimeState,
    device_name: Option<String>,
    epoch: u64,
) -> Result<u32, String> {
    let lease = state.audio_session.acquire(AudioOwner::Comparison)?;
    // 这几步任何一步失败都必须归还租约：AudioLease 没有 Drop，泄漏意味着音频独占权
    // 一直挂在「模型对比」名下。
    let started = (|| -> Result<_, String> {
        state.audio_session.attach(&lease, "comparison")?;
        let mic = start_backend_mic_inner(device_name, state)?;
        let (_, receiver) = attach_backend_mic_raw_inner(state)?;
        Ok((mic, receiver))
    })();
    let (mic, mut receiver) = match started {
        Ok(value) => value,
        Err(error) => {
            let _ = release_backend_mic_inner(state);
            let _ = state.audio_session.release(&lease);
            return Err(error);
        }
    };
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
        let capture_error = app
            .state::<RuntimeState>()
            .backend_mic
            .lock()
            .ok()
            .and_then(|mut capture| capture.last_error.take());
        if let Some(error) = capture_error {
            fail_recording_capture(&app, epoch, error);
        }
    });
    Ok(mic.sample_rate)
}

fn fail_recording_capture(app: &tauri::AppHandle, epoch: u64, error: String) {
    let state = app.state::<RuntimeState>();
    let (sessions, lease) = {
        let Ok(mut compare) = state.compare_runtime.inner.lock() else {
            return;
        };
        if state.compare_runtime.epoch.load(Ordering::Acquire) != epoch
            || compare.phase != "recording"
        {
            return;
        }
        compare.phase = "idle".into();
        compare.error = error.clone();
        compare.recording_drain = None;
        compare.playback_progress = None;
        for cell in &mut compare.cells {
            if !matches!(cell.status.as_str(), "done" | "error") {
                cell.status = "error".into();
                cell.error_message = error.clone();
            }
        }
        (std::mem::take(&mut compare.sessions), compare.lease.take())
    };
    let _ = release_backend_mic_inner(&state);
    for (session_id, _) in sessions {
        let _ = stop_asr_stream_inner(&session_id, &state);
    }
    if let Some(lease) = lease {
        let _ = state.audio_session.release(&lease);
    }
    publish(app);
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
        match transcription_start_inner(app.clone(), state, path.clone(), Some(params), "compare").await {
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
                let text = payload.get("text").and_then(Value::as_str).unwrap_or_default();
                let is_final = payload.get("final").and_then(Value::as_bool) == Some(true);
                state.compare_runtime.update_streaming(index, text, is_final);
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
            ..Default::default()
        }]);
        assert_eq!(runtime.domain_snapshot().state, DomainRunState::Running);
        runtime.update_cell(3, "done", Some("结果".into()), None);
        let snapshot = runtime.snapshot();
        assert_eq!(snapshot.cells[0].index, 3);
        assert_eq!(snapshot.cells[0].text, "结果");
    }

    /// 启动中途失败必须把现场收干净，否则整个模型对比功能会一直卡死。
    ///
    /// 此前 `compare_start` 用裸 `?` 直接返回：已经建好的实时 ASR 会话不会被回收
    /// （每个各占一个 worker 线程，本地 sherpa 还会把 200MB+ 权重钉在内存），而
    /// `phase` 永远停在 `starting`，用户此后再点「开始对比」只会得到「模型对比正在
    /// 运行」，只能重启应用。
    #[test]
    fn aborting_a_failed_start_releases_sessions_and_unblocks_the_next_run() {
        let runtime = CompareRuntime::default();
        runtime.reset(vec![
            CompareCellSnapshot {
                index: 0,
                status: "connecting".into(),
                ..Default::default()
            },
            CompareCellSnapshot {
                index: 1,
                status: "done".into(),
                text: "已完成".into(),
                ..Default::default()
            },
        ]);
        {
            let mut state = runtime.inner.lock().unwrap();
            state.sessions.insert("session-a".into(), 0);
            state.lease = Some(AudioLease {
                owner: AudioOwner::Comparison,
                generation: 3,
            });
        }
        assert_eq!(runtime.domain_snapshot().state, DomainRunState::Running);

        let (sessions, lease) = runtime.abort("麦克风被占用");

        // 交出去的会话与租约，调用方才有机会真正关掉它们。
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions.get("session-a"), Some(&0));
        assert!(lease.is_some());

        let snapshot = runtime.snapshot();
        assert_eq!(snapshot.phase, "idle", "phase 必须回到 idle，否则下一次启动被拒");
        assert_eq!(snapshot.error, "麦克风被占用");
        assert_eq!(snapshot.cells[0].status, "error");
        assert_eq!(snapshot.cells[0].error_message, "麦克风被占用");
        // 已经出结果的格子不该被抹掉。
        assert_eq!(snapshot.cells[1].status, "done");
        assert_eq!(snapshot.cells[1].text, "已完成");
        assert_eq!(runtime.domain_snapshot().state, DomainRunState::Idle);

        // 状态里不再残留会话与租约，重复 abort 也是安全的。
        let (again, lease_again) = runtime.abort("再次");
        assert!(again.is_empty());
        assert!(lease_again.is_none());
    }

    /// `compare_start` 在 `reset` 之后不得再出现裸 `?`：任何一步失败都要走 abort_start。
    #[test]
    fn compare_start_routes_every_failure_through_abort() {
        // 归一化行尾：按 core.autocrlf 检出时工作区是 CRLF，含 \n 的切片会失配。
        let source = include_str!("compare.rs").replace("\r\n", "\n");
        let body = &source[..source
            .find("#[cfg(test)]")
            .expect("compare.rs 必须有测试模块标记")];
        let start = body
            .find("pub(crate) async fn compare_start")
            .expect("compare_start 必须仍然存在");
        let command = &body[start..];
        let command = &command[..command.find("\n}\n").expect("函数体未闭合")];
        let after_reset = &command[command
            .find("reset(cells)")
            .expect("compare_start 必须仍然先 reset")..];

        assert!(
            after_reset.contains("abort_start("),
            "reset 之后的失败必须经过 abort_start 收回现场"
        );
        assert!(
            !after_reset.contains("?;"),
            "reset 之后不得再用裸 ?：那会把已建立的 ASR 会话与租约留在原地，phase 卡在 starting"
        );
    }

    /// 实时流的 `result.text` 只是当前这一句。此前 `handle_event` 无视 `final`
    /// 直接整体替换单元格文本，于是一段多句录音跑完只剩最后一句。
    #[test]
    fn streaming_keeps_every_finalized_sentence() {
        let runtime = CompareRuntime::default();
        runtime.reset(vec![CompareCellSnapshot {
            index: 0,
            status: "queued".into(),
            ..Default::default()
        }]);
        runtime.update_streaming(0, "第一句", false);
        runtime.update_streaming(0, "第一句话。", true);
        runtime.update_streaming(0, "第二句", false);
        assert_eq!(runtime.snapshot().cells[0].text, "第一句话。第二句");
        runtime.update_streaming(0, "第二句话。", true);
        runtime.update_streaming(0, "第三句", false);
        assert_eq!(
            runtime.snapshot().cells[0].text,
            "第一句话。第二句话。第三句"
        );
    }

    /// 录音模式的实时流采样率必须来自麦克风，不能在麦克风启动前猜一个 48k：
    /// 44.1k 设备上那等于把音频按 48k 解读，整段加速 8.8%。这里只能做源码契约
    /// 校验——真正跑一遍需要物理麦克风。
    #[test]
    fn record_mode_takes_the_sample_rate_from_the_microphone() {
        // 归一化行尾：按 core.autocrlf 检出时工作区是 CRLF，含 \n 的切片会失配。
        let source = include_str!("compare.rs").replace("\r\n", "\n");
        let body = &source[..source
            .find("#[cfg(test)]")
            .expect("compare.rs 必须有测试模块标记")];
        let start = body
            .find("let realtime_sample_rate")
            .expect("compare_start 必须仍然决定 realtime_sample_rate");
        let binding = &body[start..];
        let binding = &binding[..binding.find("\n    };").expect("绑定表达式未闭合")];
        assert!(
            binding.contains("start_recording("),
            "录音模式必须先启动麦克风、用它返回的真实采样率去开实时流"
        );
        assert!(
            !binding.contains("48_000"),
            "录音模式不得硬编码 48kHz 输入采样率"
        );
    }
}
