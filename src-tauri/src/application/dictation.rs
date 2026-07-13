use crate::application::audio_session::{AudioLease, AudioOwner};
use crate::application::contract::{next_revision, DomainEventEnvelope};
use crate::application::events::BackendEvent;
use crate::commands::asr::{
    asr_stream_finish_inner, start_asr_stream_inner, stop_asr_stream_inner,
};
use crate::commands::dictation::inject_text_inner;
use crate::commands::transcription::{transcription_cancel_inner, transcription_start_inner};
use crate::desktop::{
    attach_backend_mic_raw_inner, attach_backend_mic_to_asr_inner, pause_backend_mic_inner,
    prepare_dictation_indicator, release_backend_mic_inner, start_backend_mic_inner,
};
use crate::prelude::*;
use crate::providers::alibabacloud::TranscriptionParams;
use crate::state::{AsrStreamInput, RuntimeState};
use fancy_regex::{Captures, RegexBuilder};
use std::sync::atomic::{AtomicU64, Ordering};
use tauri::AppHandle;

const DOMAIN_EVENT: &str = "domain-event";
const FINALIZE_TIMEOUT_MS: u64 = 8_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum DictationPhase {
    Idle,
    WaitingForVoice,
    Recording,
    Finishing,
    ProcessingFile,
    Injecting,
    Failed,
}

impl Default for DictationPhase {
    fn default() -> Self {
        Self::Idle
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DictationMode {
    Realtime,
    File,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LocalRule {
    #[serde(default)]
    id: String,
    #[serde(default)]
    enabled: bool,
    #[serde(default)]
    mode: String,
    #[serde(default)]
    find: String,
    #[serde(default)]
    pattern: String,
    #[serde(default = "default_flags")]
    flags: String,
    #[serde(default)]
    replacement: String,
}
fn default_flags() -> String {
    "g".into()
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct DictationPrefs {
    asr_model: String,
    keep_alive_ms: u64,
    cue_enabled: bool,
    cue_start: String,
    cue_end: String,
    local_rules_enabled: bool,
    local_rules: Vec<LocalRule>,
    mic_device_id: String,
    dictation_silence_disconnect_enabled: bool,
    dictation_silence_disconnect_ms: u64,
    dictation_silence_threshold: f32,
    #[serde(flatten)]
    dsp: DspParams,
}
impl Default for DictationPrefs {
    fn default() -> Self {
        Self {
            asr_model: crate::providers::registry::default_realtime_model().into(),
            keep_alive_ms: 60_000,
            cue_enabled: true,
            cue_start: "beep-up".into(),
            cue_end: "beep-down".into(),
            local_rules_enabled: false,
            local_rules: vec![],
            mic_device_id: String::new(),
            dictation_silence_disconnect_enabled: true,
            dictation_silence_disconnect_ms: 5_000,
            dictation_silence_threshold: 0.0001,
            dsp: DspParams::default(),
        }
    }
}

#[derive(Default)]
struct Session {
    epoch: u64,
    phase: DictationPhase,
    mode: Option<DictationMode>,
    public_id: Option<String>,
    asr_session_id: Option<String>,
    file_job_id: Option<String>,
    committed: String,
    segment: String,
    raw_samples: Vec<f32>,
    sample_rate: u32,
    injected_epoch: Option<u64>,
    lease: Option<AudioLease>,
    prefs: DictationPrefs,
    last_voice_at: Option<Instant>,
    silence_streaming: bool,
    raw_done: Option<Arc<tokio::sync::Notify>>,
    temp_audio_path: Option<PathBuf>,
}

impl Session {
    fn is_current(&self, epoch: u64) -> bool {
        self.epoch == epoch
    }

    fn claim_injection(&mut self, epoch: u64) -> bool {
        if !self.is_current(epoch) || self.injected_epoch == Some(epoch) {
            return false;
        }
        self.injected_epoch = Some(epoch);
        true
    }
}

pub(crate) struct DictationRuntime {
    session: Arc<Mutex<Session>>,
    operation: Arc<tokio::sync::Mutex<()>>,
    epochs: AtomicU64,
}
impl Default for DictationRuntime {
    fn default() -> Self {
        Self {
            session: Arc::new(Mutex::new(Session::default())),
            operation: Arc::new(tokio::sync::Mutex::new(())),
            epochs: AtomicU64::new(0),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DictationSnapshot {
    phase: DictationPhase,
    session_id: Option<String>,
    text: String,
    error: Option<String>,
}

pub(crate) fn initialize(app: AppHandle) {
    let mut receiver = app.state::<RuntimeState>().backend_events.subscribe();
    tauri::async_runtime::spawn(async move {
        loop {
            match receiver.recv().await {
                Ok(event) => handle_backend_event(app.clone(), event).await,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(count)) => {
                    dlog!("[dictation] 后端事件积压，跳过 {count} 条")
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

pub(crate) fn request_toggle(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        if let Err(e) = toggle(app.clone()).await {
            publish_state(&app, Some(e));
        }
    });
}
pub(crate) fn request_start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        if let Err(e) = start(app.clone()).await {
            publish_state(&app, Some(e));
        }
    });
}
pub(crate) fn request_stop(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        if let Err(e) = stop(app.clone()).await {
            publish_state(&app, Some(e));
        }
    });
}
pub(crate) fn request_cancel(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        if let Err(e) = cancel(app.clone()).await {
            publish_state(&app, Some(e));
        }
    });
}

#[tauri::command]
pub(crate) async fn dictation_toggle(app: AppHandle) -> Result<(), String> {
    toggle(app).await
}
#[tauri::command]
pub(crate) async fn dictation_start(app: AppHandle) -> Result<(), String> {
    start(app).await
}
#[tauri::command]
pub(crate) async fn dictation_stop(app: AppHandle) -> Result<(), String> {
    stop(app).await
}
#[tauri::command]
pub(crate) async fn dictation_cancel(app: AppHandle) -> Result<(), String> {
    cancel(app).await
}
#[tauri::command]
pub(crate) fn get_dictation_runtime(
    state: tauri::State<'_, RuntimeState>,
) -> Result<DictationSnapshot, String> {
    snapshot(&state)
}

#[tauri::command]
pub(crate) fn preview_dictation_cue(
    app: AppHandle,
    state: tauri::State<'_, RuntimeState>,
    which: String,
) -> Result<(), String> {
    if which != "start" && which != "end" {
        return Err("未知提示音位置".into());
    }
    let prefs: DictationPrefs = serde_json::from_value(
        state
            .app_settings
            .lock()
            .map_err(|_| "应用配置锁失败")?
            .dictation_prefs
            .clone(),
    )
    .map_err(|e| format!("听写配置无效：{e}"))?;
    play_cue_async(app, if which == "start" { "start" } else { "end" }, &prefs);
    Ok(())
}

fn snapshot(state: &RuntimeState) -> Result<DictationSnapshot, String> {
    let s = state
        .dictation_runtime
        .session
        .lock()
        .map_err(|_| "听写状态锁失败")?;
    Ok(DictationSnapshot {
        phase: s.phase,
        session_id: s.public_id.clone(),
        text: format!("{}{}", s.committed, s.segment),
        error: None,
    })
}

pub(crate) fn domain_snapshot(
    state: &RuntimeState,
) -> Result<crate::application::contract::DomainSnapshot, String> {
    use crate::application::contract::{DomainRunState, DomainSnapshot};
    let s = state
        .dictation_runtime
        .session
        .lock()
        .map_err(|_| "听写状态锁失败")?;
    let run_state = match s.phase {
        DictationPhase::Idle => DomainRunState::Idle,
        DictationPhase::Finishing | DictationPhase::ProcessingFile | DictationPhase::Injecting => {
            DomainRunState::Stopping
        }
        DictationPhase::Failed => DomainRunState::Failed,
        DictationPhase::WaitingForVoice | DictationPhase::Recording => DomainRunState::Running,
    };
    Ok(DomainSnapshot {
        state: run_state,
        session_id: s.public_id.clone(),
    })
}

async fn toggle(app: AppHandle) -> Result<(), String> {
    let phase = app
        .state::<RuntimeState>()
        .dictation_runtime
        .session
        .lock()
        .map_err(|_| "听写状态锁失败")?
        .phase;
    match phase {
        DictationPhase::Idle | DictationPhase::Failed => start(app).await,
        DictationPhase::WaitingForVoice | DictationPhase::Recording => stop(app).await,
        _ => Ok(()),
    }
}

async fn start(app: AppHandle) -> Result<(), String> {
    let state = app.state::<RuntimeState>();
    let operation = state.dictation_runtime.operation.clone();
    let _guard = operation.lock().await;
    let phase = state
        .dictation_runtime
        .session
        .lock()
        .map_err(|_| "听写状态锁失败")?
        .phase;
    if !matches!(phase, DictationPhase::Idle | DictationPhase::Failed) {
        return Ok(());
    }
    let prefs_value = state
        .app_settings
        .lock()
        .map_err(|_| "应用配置锁失败")?
        .dictation_prefs
        .clone();
    let prefs: DictationPrefs =
        serde_json::from_value(prefs_value).map_err(|e| format!("听写配置无效：{e}"))?;
    validate_rules(&prefs)?;
    let epoch = state
        .dictation_runtime
        .epochs
        .fetch_add(1, Ordering::AcqRel)
        + 1;
    let info = crate::providers::registry::model_info(&prefs.asr_model)
        .ok_or_else(|| format!("听写模型未登记：{}", prefs.asr_model))?;
    let mode = if info.scenes.iter().any(|s| s == "dictationFile") {
        DictationMode::File
    } else {
        DictationMode::Realtime
    };
    let lease = state.audio_session.acquire(AudioOwner::Dictation)?;
    state.audio_session.attach(&lease, "dictation")?;
    let mic = match start_backend_mic_inner(
        if prefs.mic_device_id.trim().is_empty() {
            None
        } else {
            Some(prefs.mic_device_id.clone())
        },
        &state,
    ) {
        Ok(mic) => mic,
        Err(error) => {
            let _ = state.audio_session.release(&lease);
            return Err(error);
        }
    };
    let public_id = Uuid::new_v4().to_string();
    {
        let mut s = state
            .dictation_runtime
            .session
            .lock()
            .map_err(|_| "听写状态锁失败")?;
        *s = Session {
            epoch,
            phase: if mode == DictationMode::Realtime && prefs.dictation_silence_disconnect_enabled
            {
                DictationPhase::WaitingForVoice
            } else {
                DictationPhase::Recording
            },
            mode: Some(mode),
            public_id: Some(public_id),
            sample_rate: mic.sample_rate,
            lease: Some(lease),
            prefs: prefs.clone(),
            silence_streaming: false,
            ..Session::default()
        };
    }
    let (_, raw_rx) = match attach_backend_mic_raw_inner(&state) {
        Ok(value) => value,
        Err(error) => {
            cleanup_failed_start(&state, epoch);
            return Err(error);
        }
    };
    let raw_done = Arc::new(tokio::sync::Notify::new());
    if let Ok(mut s) = state.dictation_runtime.session.lock() {
        if s.epoch == epoch {
            s.raw_done = Some(raw_done.clone());
        }
    }
    spawn_raw_consumer(app.clone(), epoch, raw_rx, raw_done);
    if mode == DictationMode::Realtime && !prefs.dictation_silence_disconnect_enabled {
        if let Err(error) = open_asr(app.clone(), epoch).await {
            cleanup_failed_start(&state, epoch);
            return Err(error);
        }
    }
    play_cue_async(app.clone(), "start", &prefs);
    publish_state(&app, None);
    let _ = prepare_dictation_indicator(&app);
    let _ = crate::desktop::set_indicator_layout(
        app.clone(),
        Some(460.0),
        Some(188.0),
        Some("bottom".into()),
        Some(36.0),
    );
    let _ = crate::desktop::set_indicator_state(app, "recording".into());
    Ok(())
}

fn cleanup_failed_start(state: &RuntimeState, epoch: u64) {
    let lease = state
        .dictation_runtime
        .session
        .lock()
        .ok()
        .and_then(|mut s| {
            if s.epoch != epoch {
                return None;
            }
            s.phase = DictationPhase::Failed;
            s.mode = None;
            s.public_id = None;
            s.lease.take()
        });
    let _ = release_backend_mic_inner(state);
    if let Some(lease) = lease {
        let _ = state.audio_session.release(&lease);
    }
}

fn spawn_raw_consumer(
    app: AppHandle,
    epoch: u64,
    mut rx: tokio::sync::mpsc::UnboundedReceiver<AsrStreamInput>,
    done: Arc<tokio::sync::Notify>,
) {
    tauri::async_runtime::spawn(async move {
        while let Some(input) = rx.recv().await {
            let AsrStreamInput::RawF32(samples) = input else {
                continue;
            };
            let (need_open, need_close, peaks, level) = {
                let state = app.state::<RuntimeState>();
                let Ok(mut s) = state.dictation_runtime.session.lock() else {
                    break;
                };
                if s.epoch != epoch
                    || !matches!(
                        s.phase,
                        DictationPhase::Recording
                            | DictationPhase::WaitingForVoice
                            | DictationPhase::ProcessingFile
                    )
                {
                    break;
                }
                if s.mode == Some(DictationMode::File) {
                    s.raw_samples.extend_from_slice(&samples);
                }
                let level = rms(&samples);
                let peaks = summarize_peaks(&samples, 6);
                let mut need_open = false;
                let mut need_close = false;
                if s.mode == Some(DictationMode::Realtime)
                    && s.prefs.dictation_silence_disconnect_enabled
                {
                    if level > s.prefs.dictation_silence_threshold {
                        s.last_voice_at = Some(Instant::now());
                        if !s.silence_streaming {
                            s.silence_streaming = true;
                            s.phase = DictationPhase::Recording;
                            need_open = true;
                        }
                    } else if s.silence_streaming
                        && s.last_voice_at
                            .map(|v| {
                                v.elapsed()
                                    >= Duration::from_millis(
                                        s.prefs.dictation_silence_disconnect_ms,
                                    )
                            })
                            .unwrap_or(false)
                    {
                        s.silence_streaming = false;
                        s.phase = DictationPhase::WaitingForVoice;
                        need_close = true;
                    }
                }
                (need_open, need_close, peaks, level)
            };
            emit_waveform(&app, level, peaks);
            if need_close {
                disconnect_silent_asr(app.clone(), epoch);
            }
            if need_open {
                let _ = open_asr(app.clone(), epoch).await;
            }
        }
        done.notify_one();
    });
}

async fn open_asr(app: AppHandle, epoch: u64) -> Result<(), String> {
    let (model, rate, dsp) = {
        let state = app.state::<RuntimeState>();
        let s = state
            .dictation_runtime
            .session
            .lock()
            .map_err(|_| "听写状态锁失败")?;
        if s.epoch != epoch || s.asr_session_id.is_some() {
            return Ok(());
        }
        (
            s.prefs.asr_model.clone(),
            s.sample_rate,
            s.prefs.dsp.clone(),
        )
    };
    let state = app.state::<RuntimeState>();
    let response = start_asr_stream_inner(
        app.clone(),
        &state,
        None,
        Some(model),
        Some(rate),
        Some(dsp),
    )
    .await?;
    if let Err(error) = attach_backend_mic_to_asr_inner(&response.session_id, &state) {
        let _ = stop_asr_stream_inner(&response.session_id, &state);
        return Err(error);
    }
    let mut s = state
        .dictation_runtime
        .session
        .lock()
        .map_err(|_| "听写状态锁失败")?;
    if s.epoch != epoch {
        stop_asr_stream_inner(&response.session_id, &state)?;
        return Ok(());
    }
    s.asr_session_id = Some(response.session_id);
    drop(s);
    publish_state(&app, None);
    Ok(())
}

fn disconnect_silent_asr(app: AppHandle, epoch: u64) {
    let state = app.state::<RuntimeState>();
    let session = state
        .dictation_runtime
        .session
        .lock()
        .ok()
        .and_then(|mut s| {
            if s.epoch == epoch {
                s.segment.clear();
                s.asr_session_id.take()
            } else {
                None
            }
        });
    if let Some(id) = session {
        let _ = stop_asr_stream_inner(&id, &state);
    }
    publish_state(&app, None);
}

async fn stop(app: AppHandle) -> Result<(), String> {
    let state = app.state::<RuntimeState>();
    let operation = state.dictation_runtime.operation.clone();
    let _guard = operation.lock().await;
    let (epoch, mode, session_id, rate, prefs, raw_done, audio_generation) = {
        let mut s = state
            .dictation_runtime
            .session
            .lock()
            .map_err(|_| "听写状态锁失败")?;
        if !matches!(
            s.phase,
            DictationPhase::Recording | DictationPhase::WaitingForVoice
        ) {
            return Ok(());
        }
        s.phase = if s.mode == Some(DictationMode::File) {
            DictationPhase::ProcessingFile
        } else {
            DictationPhase::Finishing
        };
        (
            s.epoch,
            s.mode,
            s.asr_session_id.clone(),
            s.sample_rate,
            s.prefs.clone(),
            s.raw_done.clone(),
            s.lease.as_ref().map(|v| v.generation).unwrap_or(0),
        )
    };
    pause_backend_mic_inner(&state)?;
    if mode == Some(DictationMode::File) {
        if let Some(done) = raw_done {
            let _ = tokio::time::timeout(Duration::from_secs(1), done.notified()).await;
        }
    }
    let raw = state
        .dictation_runtime
        .session
        .lock()
        .map_err(|_| "听写状态锁失败")
        .map(|mut s| std::mem::take(&mut s.raw_samples))?;
    if prefs.keep_alive_ms == 0 {
        let _ = release_backend_mic_inner(&state);
    } else {
        schedule_release(app.clone(), epoch, audio_generation, prefs.keep_alive_ms);
    }
    publish_state(&app, None);
    let _ = crate::desktop::set_indicator_state(app.clone(), "processing".into());
    let result = match mode {
        Some(DictationMode::Realtime) => {
            if let Some(id) = session_id {
                asr_stream_finish_inner(&id, &state)
                    .map(|_| spawn_finalize_timeout(app.clone(), epoch))
            } else {
                finalize(app.clone(), epoch).await;
                Ok(())
            }
        }
        Some(DictationMode::File) => start_file_job(app.clone(), epoch, raw, rate, prefs).await,
        None => Ok(()),
    };
    if let Err(error) = result {
        return fail(app, epoch, error).await;
    }
    Ok(())
}

async fn start_file_job(
    app: AppHandle,
    epoch: u64,
    raw: Vec<f32>,
    rate: u32,
    prefs: DictationPrefs,
) -> Result<(), String> {
    if raw.is_empty() {
        return Err("未录到音频".into());
    }
    let path = write_wav(raw, rate).await?;
    if let Ok(mut s) = app.state::<RuntimeState>().dictation_runtime.session.lock() {
        if s.epoch == epoch {
            s.temp_audio_path = Some(PathBuf::from(&path));
        }
    }
    let params = TranscriptionParams {
        model: prefs.asr_model,
        language_hints: vec![],
        diarization_enabled: Some(false),
        speaker_count: None,
        channel_id: None,
        special_word_filter: String::new(),
    };
    let state = app.state::<RuntimeState>();
    let response = transcription_start_inner(app.clone(), &state, path, Some(params)).await?;
    let mut s = state
        .dictation_runtime
        .session
        .lock()
        .map_err(|_| "听写状态锁失败")?;
    if s.epoch == epoch {
        s.file_job_id = Some(response.job_id);
    }
    Ok(())
}

async fn write_wav(samples: Vec<f32>, sample_rate: u32) -> Result<String, String> {
    let pcm = crate::audio_prep::f32_to_i16(&samples);
    let data_len = (pcm.len() * 2) as u32;
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
    for v in pcm {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    let path = std::env::temp_dir().join(format!("say-it-dictation-{}.wav", Uuid::new_v4()));
    tokio::fs::write(&path, bytes)
        .await
        .map_err(|e| format!("写入听写录音失败：{e}"))?;
    Ok(path.to_string_lossy().into_owned())
}

async fn cancel(app: AppHandle) -> Result<(), String> {
    let state = app.state::<RuntimeState>();
    let operation = state.dictation_runtime.operation.clone();
    let _guard = operation.lock().await;
    let (asr, file_job, lease, temp_path) = {
        let mut s = state
            .dictation_runtime
            .session
            .lock()
            .map_err(|_| "听写状态锁失败")?;
        let ids = (
            s.asr_session_id.take(),
            s.file_job_id.take(),
            s.lease.take(),
            s.temp_audio_path.take(),
        );
        s.epoch = state
            .dictation_runtime
            .epochs
            .fetch_add(1, Ordering::AcqRel)
            + 1;
        s.phase = DictationPhase::Idle;
        s.mode = None;
        s.public_id = None;
        s.committed.clear();
        s.segment.clear();
        s.raw_samples.clear();
        ids
    };
    let _ = pause_backend_mic_inner(&state);
    let _ = release_backend_mic_inner(&state);
    if let Some(id) = asr {
        let _ = stop_asr_stream_inner(&id, &state);
    }
    if let Some(id) = file_job {
        let _ = transcription_cancel_inner(&app, &state, &id);
    }
    if let Some(lease) = lease {
        let _ = state.audio_session.release(&lease);
    }
    remove_temp(temp_path);
    hotkey::set_dictation_active(false);
    let _ = crate::desktop::set_indicator_state(app.clone(), "hidden".into());
    publish_state(&app, None);
    Ok(())
}

async fn handle_backend_event(app: AppHandle, event: BackendEvent) {
    match event {
        BackendEvent::Asr {
            session_id,
            kind,
            payload,
        } => handle_asr_event(app, session_id, kind, payload).await,
        BackendEvent::Transcription {
            job_id,
            stage,
            payload,
        } => handle_file_event(app, job_id, stage, payload).await,
        BackendEvent::SubtitleTranslation { .. } => {}
    }
}

async fn handle_asr_event(app: AppHandle, session_id: String, kind: String, payload: Value) {
    let mut finalize_epoch = None;
    let mut failure = None;
    {
        let state = app.state::<RuntimeState>();
        let Ok(mut s) = state.dictation_runtime.session.lock() else {
            return;
        };
        if s.asr_session_id.as_deref() != Some(&session_id) {
            return;
        }
        match kind.as_str() {
            "result" => {
                if let Some(text) = payload.get("text").and_then(Value::as_str) {
                    s.segment = text.into();
                    if payload.get("final").and_then(Value::as_bool) == Some(true) {
                        let segment = std::mem::take(&mut s.segment);
                        s.committed.push_str(&segment);
                    }
                }
            }
            "finish" | "finish_timeout" => finalize_epoch = Some(s.epoch),
            "ended" | "closed" if s.phase == DictationPhase::Finishing => {
                finalize_epoch = Some(s.epoch)
            }
            "error" if s.phase == DictationPhase::Finishing => finalize_epoch = Some(s.epoch),
            "ended" | "closed" => {
                s.asr_session_id = None;
                if s.prefs.dictation_silence_disconnect_enabled {
                    s.phase = DictationPhase::WaitingForVoice;
                    s.silence_streaming = false;
                } else {
                    failure = Some((s.epoch, "ASR 连接意外中断".to_string()));
                }
            }
            "error" => {
                let message = payload.to_string();
                if s.prefs.dictation_silence_disconnect_enabled {
                    s.asr_session_id = None;
                    s.phase = DictationPhase::WaitingForVoice;
                    s.silence_streaming = false;
                } else {
                    failure = Some((s.epoch, message));
                }
            }
            _ => {}
        }
    }
    if let Some((epoch, error)) = failure {
        let _ = fail(app, epoch, error).await;
        return;
    }
    publish_state(
        &app,
        if kind == "error" {
            Some(payload.to_string())
        } else {
            None
        },
    );
    if let Some(epoch) = finalize_epoch {
        finalize(app, epoch).await;
    }
}

async fn handle_file_event(app: AppHandle, job_id: String, stage: String, payload: Value) {
    let epoch = {
        let state = app.state::<RuntimeState>();
        let Ok(s) = state.dictation_runtime.session.lock() else {
            return;
        };
        if s.file_job_id.as_deref() != Some(&job_id) {
            return;
        }
        s.epoch
    };
    if stage == "completed" {
        let text = payload
            .get("result")
            .and_then(|r| r.get("transcripts"))
            .and_then(Value::as_array)
            .map(|v| {
                v.iter()
                    .filter_map(|t| t.get("text").and_then(Value::as_str))
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default();
        {
            let state = app.state::<RuntimeState>();
            if let Ok(mut s) = state.dictation_runtime.session.lock() {
                if s.epoch == epoch {
                    s.committed = text;
                }
            };
        }
        finalize(app, epoch).await;
    } else if stage == "error" {
        let _ = fail(
            app,
            epoch,
            payload
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("文件识别失败")
                .into(),
        )
        .await;
    }
}

fn spawn_finalize_timeout(app: AppHandle, epoch: u64) {
    tauri::async_runtime::spawn(async move {
        sleep(Duration::from_millis(FINALIZE_TIMEOUT_MS)).await;
        finalize(app, epoch).await;
    });
}

async fn finalize(app: AppHandle, epoch: u64) {
    let (text, prefs, method, lease, asr, temp_path) = {
        let state = app.state::<RuntimeState>();
        let Ok(mut s) = state.dictation_runtime.session.lock() else {
            return;
        };
        if !s.is_current(epoch)
            || !matches!(
                s.phase,
                DictationPhase::Finishing | DictationPhase::ProcessingFile
            )
        {
            return;
        }
        if !s.claim_injection(epoch) {
            return;
        }
        s.phase = DictationPhase::Injecting;
        let method = state
            .dictation
            .lock()
            .ok()
            .map(|v| v.inject_method.clone())
            .unwrap_or_else(|| "paste".into());
        (
            format!("{}{}", s.committed, s.segment).trim().to_string(),
            s.prefs.clone(),
            method,
            s.lease.take(),
            s.asr_session_id.take(),
            s.temp_audio_path.take(),
        )
    };
    publish_state(&app, None);
    if let Some(id) = asr {
        let state = app.state::<RuntimeState>();
        let _ = stop_asr_stream_inner(&id, &state);
    }
    let processed = match apply_rules(&text, &prefs) {
        Ok(v) => v,
        Err(e) => {
            let state = app.state::<RuntimeState>();
            if let Some(lease) = &lease {
                let _ = state.audio_session.release(lease);
            }
            remove_temp(temp_path.clone());
            let _ = fail(app, epoch, e).await;
            return;
        }
    };
    let result = if processed.is_empty() {
        Ok(())
    } else {
        inject_text_inner(processed.clone(), Some(method)).await
    };
    let state = app.state::<RuntimeState>();
    if let Some(lease) = lease {
        let _ = state.audio_session.release(&lease);
    }
    remove_temp(temp_path);
    if let Err(e) = result {
        let _ = fail(app, epoch, e).await;
        return;
    }
    if let Ok(mut s) = state.dictation_runtime.session.lock() {
        if s.epoch == epoch {
            s.phase = DictationPhase::Idle;
            s.mode = None;
            s.public_id = None;
            s.file_job_id = None;
            s.committed.clear();
            s.segment.clear();
        }
    }
    hotkey::set_dictation_active(false);
    let _ = crate::desktop::set_indicator_state(app.clone(), "hidden".into());
    play_cue_async(app.clone(), "end", &prefs);
    publish_state_with_text(&app, None, processed);
}

async fn fail(app: AppHandle, epoch: u64, error: String) -> Result<(), String> {
    let state = app.state::<RuntimeState>();
    let (lease, temp_path, asr, file_job) = {
        let mut s = state
            .dictation_runtime
            .session
            .lock()
            .map_err(|_| "听写状态锁失败")?;
        if s.epoch != epoch {
            return Ok(());
        }
        s.phase = DictationPhase::Failed;
        (
            s.lease.take(),
            s.temp_audio_path.take(),
            s.asr_session_id.take(),
            s.file_job_id.take(),
        )
    };
    if let Some(lease) = lease {
        let _ = state.audio_session.release(&lease);
    }
    remove_temp(temp_path);
    if let Some(id) = asr {
        let _ = stop_asr_stream_inner(&id, &state);
    }
    if let Some(id) = file_job {
        let _ = transcription_cancel_inner(&app, &state, &id);
    }
    let _ = release_backend_mic_inner(&state);
    hotkey::set_dictation_active(false);
    let _ = crate::desktop::set_indicator_state(app.clone(), "hidden".into());
    publish_state(&app, Some(error.clone()));
    Err(error)
}

fn remove_temp(path: Option<PathBuf>) {
    if let Some(path) = path {
        tauri::async_runtime::spawn(async move {
            let _ = tokio::fs::remove_file(path).await;
        });
    }
}

fn schedule_release(app: AppHandle, epoch: u64, generation: u64, delay: u64) {
    tauri::async_runtime::spawn(async move {
        if delay > 0 {
            sleep(Duration::from_millis(delay)).await;
        }
        let state = app.state::<RuntimeState>();
        let should = state
            .dictation_runtime
            .session
            .lock()
            .map(|s| {
                s.epoch == epoch
                    && !matches!(
                        s.phase,
                        DictationPhase::Recording | DictationPhase::WaitingForVoice
                    )
            })
            .unwrap_or(false)
            && state.audio_session.can_release_device(generation);
        if should {
            let _ = release_backend_mic_inner(&state);
        }
    });
}

fn publish_state(app: &AppHandle, error: Option<String>) {
    let text = app
        .state::<RuntimeState>()
        .dictation_runtime
        .session
        .lock()
        .map(|s| format!("{}{}", s.committed, s.segment))
        .unwrap_or_default();
    publish_state_with_text(app, error, text);
}
fn publish_state_with_text(app: &AppHandle, error: Option<String>, text: String) {
    let state = app.state::<RuntimeState>();
    let Ok(s) = state.dictation_runtime.session.lock() else {
        return;
    };
    let revision = next_revision(&state.snapshot_revision);
    let event = DomainEventEnvelope {
        revision,
        domain: "dictation".into(),
        event_type: "stateChanged".into(),
        session_id: s.public_id.clone(),
        payload: json!({"phase": s.phase, "recording": matches!(s.phase, DictationPhase::Recording | DictationPhase::WaitingForVoice), "text": text, "error": error}),
    };
    hotkey::set_dictation_active(!matches!(
        s.phase,
        DictationPhase::Idle | DictationPhase::Failed
    ));
    let _ = app.emit(DOMAIN_EVENT, event);
    if !text.is_empty() {
        let _ = crate::desktop::set_indicator_text(app.clone(), text, Some(false));
    }
}

fn emit_waveform(app: &AppHandle, level: f32, peaks: Vec<f32>) {
    if let Some(w) = app.get_webview_window("dictation-indicator") {
        let _ = w.emit(
            "dictation-indicator-waveform",
            json!({"active": true, "level": level, "peaks": peaks}),
        );
    }
}
fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        0.0
    } else {
        (samples.iter().map(|v| v * v).sum::<f32>() / samples.len() as f32).sqrt()
    }
}
fn summarize_peaks(samples: &[f32], n: usize) -> Vec<f32> {
    let size = (samples.len() / n.max(1)).max(1);
    (0..n)
        .map(|i| {
            samples
                .get(i * size..((i + 1) * size).min(samples.len()))
                .unwrap_or(&[])
                .iter()
                .map(|v| v.abs())
                .fold(0.0, f32::max)
        })
        .collect()
}

fn validate_rules(prefs: &DictationPrefs) -> Result<(), String> {
    if !prefs.local_rules_enabled {
        return Ok(());
    }
    for rule in prefs.local_rules.iter().filter(|r| r.enabled) {
        compile_rule(rule)?;
    }
    Ok(())
}
pub(crate) fn validate_dictation_settings_value(value: &Value) -> Result<(), String> {
    let prefs: DictationPrefs =
        serde_json::from_value(value.clone()).map_err(|e| format!("听写配置无效：{e}"))?;
    validate_rules(&prefs)
}
fn compile_rule(rule: &LocalRule) -> Result<fancy_regex::Regex, String> {
    let pattern = if rule.mode == "find" {
        regex_escape_find(&rule.find)
    } else {
        rule.pattern.clone()
    };
    let mut prefix = String::new();
    for flag in rule.flags.chars() {
        match flag {
            'g' | 'u' => {}
            'i' => prefix.push_str("(?i)"),
            'm' => prefix.push_str("(?m)"),
            's' => prefix.push_str("(?s)"),
            other => return Err(format!("规则 {} 使用不支持的 JS 标志：{other}", rule.id)),
        }
    }
    RegexBuilder::new(&(prefix + &pattern))
        .backtrack_limit(1_000_000)
        .build()
        .map_err(|e| format!("规则 {} 不兼容：{e}", rule.id))
}
fn regex_escape_find(input: &str) -> String {
    let escaped = fancy_regex::escape(input);
    let first = input
        .chars()
        .next()
        .map(|c| c.is_ascii_alphanumeric() || c == '_')
        .unwrap_or(false);
    let last = input
        .chars()
        .last()
        .map(|c| c.is_ascii_alphanumeric() || c == '_')
        .unwrap_or(false);
    format!(
        "{}{}{}",
        if first { "\\b" } else { "" },
        escaped,
        if last { "\\b" } else { "" }
    )
}
fn apply_rules(text: &str, prefs: &DictationPrefs) -> Result<String, String> {
    if !prefs.local_rules_enabled {
        return Ok(text.into());
    }
    let mut out = text.to_string();
    for rule in prefs.local_rules.iter().filter(|r| r.enabled) {
        if rule.mode == "find" && rule.find.is_empty()
            || rule.mode != "find" && rule.pattern.is_empty()
        {
            continue;
        }
        let regex = compile_rule(rule)?;
        let global = rule.flags.contains('g') || rule.mode == "find";
        let source = out.clone();
        let mut next = String::new();
        let mut last = 0;
        for found in regex.captures_iter(&source) {
            let caps = found.map_err(|e| format!("规则 {} 执行失败：{e}", rule.id))?;
            let m = caps.get(0).unwrap();
            next.push_str(&source[last..m.start()]);
            if rule.id == "dedupe-punct" {
                next.push_str(&normalize_punctuation(m.as_str()));
            } else {
                next.push_str(&expand_replacement(&rule.replacement, &caps, &source));
            }
            last = m.end();
            if !global {
                break;
            }
        }
        next.push_str(&source[last..]);
        out = next;
    }
    Ok(out.trim().to_string())
}
fn normalize_punctuation(value: &str) -> String {
    let compact = value.replace([' ', '\t'], "");
    if compact.contains('…') {
        let mut out = String::new();
        if compact.contains("……") {
            out.push_str("……")
        } else {
            out.push('…')
        }
        if compact.contains('？') || compact.contains('?') {
            out.push('？')
        }
        if compact.contains('！') || compact.contains('!') {
            out.push('！')
        }
        return out;
    }
    if compact.contains('？') || compact.contains('?') {
        let mut out = "？".to_string();
        if compact.contains('！') || compact.contains('!') {
            out.push('！')
        }
        return out;
    }
    if compact.contains('！') || compact.contains('!') {
        return "！".into();
    }
    compact.chars().last().map(String::from).unwrap_or_default()
}
fn expand_replacement(replacement: &str, caps: &Captures<'_>, source: &str) -> String {
    let whole = caps.get(0).unwrap();
    let chars: Vec<char> = replacement.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '$' || i + 1 >= chars.len() {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        match chars[i + 1] {
            '$' => {
                out.push('$');
                i += 2
            }
            '&' => {
                out.push_str(whole.as_str());
                i += 2
            }
            '`' => {
                out.push_str(&source[..whole.start()]);
                i += 2
            }
            '\'' => {
                out.push_str(&source[whole.end()..]);
                i += 2
            }
            '<' => {
                if let Some(end) = chars[i + 2..].iter().position(|c| *c == '>') {
                    let end = i + 2 + end;
                    let name: String = chars[i + 2..end].iter().collect();
                    if let Some(m) = caps.name(&name) {
                        out.push_str(m.as_str())
                    }
                    i = end + 1
                } else {
                    out.push('$');
                    i += 1
                }
            }
            c if c.is_ascii_digit() => {
                let mut j = i + 1;
                let mut n = 0usize;
                while j < chars.len() && j < i + 3 && chars[j].is_ascii_digit() {
                    n = n * 10 + chars[j].to_digit(10).unwrap() as usize;
                    j += 1;
                }
                if let Some(m) = caps.get(n) {
                    out.push_str(m.as_str())
                }
                i = j
            }
            _ => {
                out.push('$');
                i += 1
            }
        }
    }
    out
}

fn play_cue_async(app: AppHandle, which: &'static str, prefs: &DictationPrefs) {
    if !prefs.cue_enabled {
        return;
    }
    let kind = if which == "start" {
        prefs.cue_start.clone()
    } else {
        prefs.cue_end.clone()
    };
    if kind == "none" {
        return;
    }
    let custom = if kind == "custom" {
        let state = app.state::<RuntimeState>();
        state.app_settings.lock().ok().and_then(|s| {
            if which == "start" {
                s.custom_cue_start.clone()
            } else {
                s.custom_cue_end.clone()
            }
        })
    } else {
        None
    };
    tauri::async_runtime::spawn_blocking(move || {
        let result = (|| {
            let samples = if kind == "custom" {
                let file = custom.ok_or_else(|| format!("未配置{which}自定义提示音"))?;
                let dir = app.path().app_local_data_dir().map_err(|e| e.to_string())?;
                crate::audio_prep::decode_to_mono_16k(
                    dir.join(file.relative_path).to_string_lossy().as_ref(),
                )?
            } else {
                cue_samples(&kind, 16_000)
            };
            play_samples(samples, 16_000)
        })();
        if let Err(error) = result {
            let state = app.state::<RuntimeState>();
            let revision = next_revision(&state.snapshot_revision);
            let _ = app.emit(
                DOMAIN_EVENT,
                DomainEventEnvelope {
                    revision,
                    domain: "dictation".into(),
                    event_type: "cueError".into(),
                    session_id: None,
                    payload: json!({"error":error}),
                },
            );
        }
    });
}
fn cue_samples(kind: &str, rate: u32) -> Vec<f32> {
    let (freqs, dur, gap): (&[f32], f32, f32) = match kind {
        "beep-up" => (&[660., 990.], 0.10, 0.02),
        "beep-down" => (&[880., 520.], 0.12, 0.02),
        "beep-double" => (&[880., 880.], 0.07, 0.05),
        _ => (&[770.], 0.12, 0.02),
    };
    let mut out = Vec::new();
    for f in freqs {
        let n = (rate as f32 * dur) as usize;
        for i in 0..n {
            let t = i as f32 / rate as f32;
            out.push((t * f * std::f32::consts::TAU).sin() * legacy_cue_envelope(t, dur));
        }
        out.extend(std::iter::repeat(0.0).take((rate as f32 * gap) as usize));
    }
    out
}

/// 与迁移前 Web Audio 版本保持同一组时长、频率和指数音量包络；
/// 只把输出设备从 WebView 改为原生 CPAL，保证主窗口销毁后仍能播放。
fn legacy_cue_envelope(t: f32, dur: f32) -> f32 {
    const FLOOR: f32 = 0.0001;
    const PEAK: f32 = 0.25;
    const ATTACK: f32 = 0.012;
    if t <= ATTACK {
        FLOOR * (PEAK / FLOOR).powf((t / ATTACK).clamp(0.0, 1.0))
    } else {
        PEAK * (FLOOR / PEAK).powf(((t - ATTACK) / (dur - ATTACK)).clamp(0.0, 1.0))
    }
}
fn play_samples(samples: Vec<f32>, source_rate: u32) -> Result<(), String> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or("未找到默认音频输出设备")?;
    let config = device.default_output_config().map_err(|e| e.to_string())?;
    let rate = config.sample_rate().0;
    let data = Arc::new(crate::audio_dsp::resample_linear(
        &samples,
        source_rate,
        rate,
    ));
    let cursor = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let channels = config.channels() as usize;
    let err = |e| dlog!("[cue] 输出错误: {e}");
    macro_rules! build {
        ($ty:ty,$map:expr) => {{
            let d = data.clone();
            let c = cursor.clone();
            device
                .build_output_stream(
                    &config.clone().into(),
                    move |out: &mut [$ty], _| {
                        for frame in out.chunks_mut(channels) {
                            let i = c.fetch_add(1, Ordering::Relaxed);
                            let v = d.get(i).copied().unwrap_or(0.0);
                            for x in frame {
                                *x = $map(v);
                            }
                        }
                    },
                    err,
                    None,
                )
                .map_err(|e| e.to_string())?
        }};
    }
    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => build!(f32, |v: f32| v),
        cpal::SampleFormat::I16 => build!(i16, |v: f32| (v * i16::MAX as f32) as i16),
        cpal::SampleFormat::U16 => build!(u16, |v: f32| ((v + 1.0) * 0.5 * u16::MAX as f32) as u16),
        f => return Err(format!("不支持的输出格式: {f:?}")),
    };
    stream.play().map_err(|e| e.to_string())?;
    std::thread::sleep(Duration::from_secs_f32(
        data.len() as f32 / rate as f32 + 0.05,
    ));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn epoch_and_single_injection_are_explicit() {
        let runtime = DictationRuntime::default();
        assert_eq!(runtime.epochs.fetch_add(1, Ordering::AcqRel) + 1, 1);
        let mut s = runtime.session.lock().unwrap();
        s.epoch = 1;
        assert!(s.claim_injection(1));
        assert!(!s.claim_injection(1));
    }
    #[test]
    fn rules_support_backrefs_and_lookaround() {
        let p = DictationPrefs {
            local_rules_enabled: true,
            local_rules: vec![LocalRule {
                id: "a".into(),
                enabled: true,
                mode: "".into(),
                find: "".into(),
                pattern: "(?<=中)([A-Z])\\1".into(),
                flags: "g".into(),
                replacement: "$1".into(),
            }],
            ..Default::default()
        };
        assert_eq!(apply_rules("中AA", &p).unwrap(), "中A");
    }
    #[test]
    fn stale_epoch_does_not_match() {
        let mut s = Session::default();
        s.epoch = 2;
        assert!(!s.is_current(1));
    }
    #[test]
    fn builtin_cues_keep_legacy_durations_and_exponential_envelope() {
        assert_eq!(cue_samples("beep-up", 1_000).len(), 240);
        assert_eq!(cue_samples("beep-down", 1_000).len(), 280);
        assert_eq!(cue_samples("beep-double", 1_000).len(), 240);
        assert!(legacy_cue_envelope(0.012, 0.1) > 0.249);
        assert!(legacy_cue_envelope(0.1, 0.1) < 0.001);
    }
}
