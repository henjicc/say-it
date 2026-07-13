use crate::application::audio_session::{AudioLease, AudioOwner};
use crate::application::contract::{
    next_revision, DomainEventEnvelope, DomainRunState, DomainSnapshot,
};
use crate::application::events::BackendEvent;
use crate::commands::asr::{start_asr_stream_inner, stop_asr_stream_inner};
use crate::commands::common::{read_provider_settings, resolve_provider_id};
use crate::commands::obs::{sync_obs_overlay_layout, ObsOverlayLayoutRequest};
use crate::desktop::{
    attach_backend_mic_raw_inner, attach_backend_mic_to_asr_inner,
    attach_backend_system_audio_raw_inner, attach_backend_system_audio_to_asr_inner,
    pause_backend_mic_inner, pause_backend_system_audio_inner, release_backend_mic_inner,
    release_backend_system_audio_inner, start_backend_mic_inner, start_backend_system_audio_inner,
};
use crate::obs_overlay::{
    overlay_status, publish_overlay_snapshot, ObsOverlaySnapshot, ObsOverlayStyle,
};
use crate::prelude::*;
use crate::providers::capabilities::translation_for_with_plugin;
use crate::state::{AsrStreamInput, RuntimeState};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tauri::AppHandle;

const DOMAIN_EVENT: &str = "domain-event";
const REPLACE_CONTINUE_GAP: Duration = Duration::from_millis(2_500);
const MAX_TEXT_CHARS: usize = 1_800;
const MAX_RECONNECT_ATTEMPTS: u32 = 6;
const CLAUSE_MAX_CHARS: usize = 60;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum SubtitlePhase {
    #[default]
    Idle,
    WaitingForVoice,
    Running,
    Reconnecting,
    Stopping,
    Failed,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum SourceKind {
    #[default]
    Mic,
    System,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct SubtitlePrefs {
    source: String,
    asr_model: String,
    mode: String,
    font_family: String,
    font_size_percent: f64,
    line_count: u32,
    width_percent: f64,
    anchor: String,
    offset_y_percent: f64,
    text_color: String,
    background_color: String,
    background_opacity: f64,
    rounded: u32,
    motion_enabled: bool,
    motion_duration_ms: u32,
    motion_easing: String,
    fade_enabled: bool,
    fade_duration_ms: u32,
    fade_easing: String,
    translation_model: String,
    translation_source_lang: String,
    translation_target_lang: String,
    translation_layout: String,
    translation_order: String,
    obs_output_enabled: bool,
}

impl Default for SubtitlePrefs {
    fn default() -> Self {
        Self {
            source: "mic:default".into(),
            asr_model: crate::providers::registry::default_realtime_model().into(),
            mode: "replace".into(),
            font_family: "Microsoft YaHei".into(),
            font_size_percent: 2.6,
            line_count: 1,
            width_percent: 46.0,
            anchor: "bottom".into(),
            offset_y_percent: 6.0,
            text_color: "#ffffff".into(),
            background_color: "#05070a".into(),
            background_opacity: 72.0,
            rounded: 18,
            motion_enabled: false,
            motion_duration_ms: 120,
            motion_easing: "ease-out".into(),
            fade_enabled: false,
            fade_duration_ms: 180,
            fade_easing: "ease-out".into(),
            translation_model: "none".into(),
            translation_source_lang: "auto".into(),
            translation_target_lang: "zh".into(),
            translation_layout: "bilingual".into(),
            translation_order: "translationFirst".into(),
            obs_output_enabled: false,
        }
    }
}

impl SubtitlePrefs {
    fn source(&self) -> (SourceKind, Option<String>) {
        let (kind, device) = self.source.split_once(':').unwrap_or(("mic", "default"));
        let device = (device != "default" && !device.trim().is_empty()).then(|| device.to_string());
        (
            if kind == "system" {
                SourceKind::System
            } else {
                SourceKind::Mic
            },
            device,
        )
    }

    fn translation_enabled(&self) -> bool {
        !self.translation_model.trim().is_empty() && self.translation_model != "none"
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct AudioPrefs {
    subtitle_silence_disconnect_enabled: bool,
    subtitle_silence_disconnect_ms: u64,
    subtitle_silence_threshold: f32,
    #[serde(flatten)]
    dsp: DspParams,
}

impl Default for AudioPrefs {
    fn default() -> Self {
        Self {
            subtitle_silence_disconnect_enabled: true,
            subtitle_silence_disconnect_ms: 5_000,
            subtitle_silence_threshold: 0.0001,
            dsp: DspParams::default(),
        }
    }
}

#[derive(Default)]
struct SubtitleDocument {
    committed: Vec<String>,
    current: String,
    replace_line: String,
    replace_line_at: Option<Instant>,
}

impl SubtitleDocument {
    fn on_partial(&mut self, text: String, mode: &str, now: Instant) {
        if self.current.is_empty()
            && mode == "replace"
            && self
                .replace_line_at
                .is_some_and(|at| now.duration_since(at) > REPLACE_CONTINUE_GAP)
        {
            self.replace_line.clear();
        }
        self.current = text;
    }

    fn commit(&mut self, mode: &str, now: Instant) {
        let text = std::mem::take(&mut self.current);
        if text.trim().is_empty() {
            return;
        }
        self.committed.push(text.clone());
        if self.committed.len() > 12 {
            self.committed.remove(0);
        }
        if mode == "replace" {
            if !self.replace_line.is_empty() {
                self.replace_line.push(' ');
            }
            self.replace_line.push_str(&text);
            self.replace_line = tail_chars(&self.replace_line, MAX_TEXT_CHARS);
            self.replace_line_at = Some(now);
        }
    }

    fn display(&self, prefs: &SubtitlePrefs) -> String {
        if prefs.mode == "replace" {
            return tail_chars(
                &match (self.replace_line.is_empty(), self.current.is_empty()) {
                    (true, _) => self.current.clone(),
                    (_, true) => self.replace_line.clone(),
                    _ => format!("{} {}", self.replace_line, self.current),
                },
                MAX_TEXT_CHARS,
            );
        }
        let keep = prefs.line_count.max(1) as usize;
        let mut lines = self
            .committed
            .iter()
            .rev()
            .take(keep)
            .cloned()
            .collect::<Vec<_>>();
        lines.reverse();
        if !self.current.is_empty() {
            if lines.len() == keep {
                lines.remove(0);
            }
            lines.push(self.current.clone());
        }
        tail_chars(&lines.join("\n"), MAX_TEXT_CHARS)
    }
}

#[derive(Default)]
struct TranslationDocument {
    next_seq: u64,
    partial_offset: usize,
    current_group: Vec<u64>,
    committed_groups: Vec<Vec<u64>>,
    replace_groups: Vec<Vec<u64>>,
    values: BTreeMap<u64, String>,
}

impl TranslationDocument {
    fn dispatch(&mut self, text: &str, final_result: bool) -> Vec<(u64, String)> {
        let mut out = vec![];
        if self.partial_offset > text.len() {
            self.partial_offset = 0;
        }
        let mut tail = &text[self.partial_offset..];
        loop {
            let Some(cut) = clause_cut(tail) else { break };
            let clause = tail[..cut].trim();
            self.partial_offset += cut;
            tail = &tail[cut..];
            if !clause.is_empty() {
                self.next_seq += 1;
                self.current_group.push(self.next_seq);
                out.push((self.next_seq, clause.to_string()));
            }
        }
        if final_result {
            self.partial_offset = text.len();
            let rest = tail.trim();
            if !rest.is_empty() {
                self.next_seq += 1;
                self.current_group.push(self.next_seq);
                out.push((self.next_seq, rest.to_string()));
            }
        }
        out
    }

    fn commit(&mut self, mode: &str, continuing_replace: bool) {
        let group = std::mem::take(&mut self.current_group);
        self.committed_groups.push(group.clone());
        if self.committed_groups.len() > 12 {
            self.committed_groups.remove(0);
        }
        if mode == "replace" {
            if continuing_replace {
                self.replace_groups.push(group);
            } else {
                self.replace_groups = vec![group];
            }
        }
        self.partial_offset = 0;
    }

    fn update(&mut self, seq: u64, text: String) {
        self.values.insert(seq, text);
    }

    fn display(&self, prefs: &SubtitlePrefs) -> String {
        let join = |group: &Vec<u64>| {
            group
                .iter()
                .filter_map(|seq| self.values.get(seq))
                .cloned()
                .collect::<String>()
        };
        let text = if prefs.mode == "replace" {
            self.replace_groups
                .iter()
                .chain(std::iter::once(&self.current_group))
                .map(join)
                .filter(|v| !v.is_empty())
                .collect::<Vec<_>>()
                .join(" ")
        } else {
            self.committed_groups
                .iter()
                .chain(std::iter::once(&self.current_group))
                .map(join)
                .filter(|v| !v.is_empty())
                .rev()
                .take(prefs.line_count.max(1) as usize)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .join("\n")
        };
        tail_chars(&text, MAX_TEXT_CHARS)
    }
}

#[derive(Default)]
struct Session {
    epoch: u64,
    public_id: Option<String>,
    phase: SubtitlePhase,
    source: SourceKind,
    sample_rate: u32,
    lease: Option<AudioLease>,
    asr_session_id: Option<String>,
    prefs: SubtitlePrefs,
    audio_prefs: AudioPrefs,
    document: SubtitleDocument,
    translation: TranslationDocument,
    last_voice_at: Option<Instant>,
    reconnect_attempts: u32,
    opening: bool,
    obs_active: bool,
    obs_disconnected_at: Option<Instant>,
    error: Option<String>,
}

impl Session {
    fn apply_translation(&mut self, epoch: u64, seq: u64, text: String) -> bool {
        if self.epoch != epoch
            || matches!(self.phase, SubtitlePhase::Idle | SubtitlePhase::Stopping)
        {
            return false;
        }
        self.translation.update(seq, text);
        true
    }
}

pub(crate) struct SubtitleRuntime {
    session: Arc<Mutex<Session>>,
    operation: Arc<tokio::sync::Mutex<()>>,
    epochs: AtomicU64,
}

impl Default for SubtitleRuntime {
    fn default() -> Self {
        Self {
            session: Arc::new(Mutex::new(Session::default())),
            operation: Arc::new(tokio::sync::Mutex::new(())),
            epochs: AtomicU64::new(0),
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SubtitleSnapshot {
    phase: SubtitlePhase,
    session_id: Option<String>,
    original_text: String,
    translation_text: String,
    obs_output_active: bool,
    error: Option<String>,
}

pub(crate) fn initialize(app: AppHandle) {
    let mut receiver = app.state::<RuntimeState>().backend_events.subscribe();
    tauri::async_runtime::spawn(async move {
        loop {
            match receiver.recv().await {
                Ok(event) => handle_backend_event(app.clone(), event).await,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(count)) => {
                    dlog!("[subtitles] 后端事件积压，跳过 {count} 条")
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

pub(crate) fn request_toggle(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        if let Err(error) = toggle(app.clone()).await {
            fail(&app, error);
        }
    });
}

#[tauri::command]
pub(crate) async fn subtitle_toggle(app: AppHandle) -> Result<(), String> {
    toggle(app).await
}

#[tauri::command]
pub(crate) async fn subtitle_stop(app: AppHandle) -> Result<(), String> {
    stop(app).await
}

#[tauri::command]
pub(crate) fn get_subtitle_runtime(
    state: tauri::State<'_, RuntimeState>,
) -> Result<SubtitleSnapshot, String> {
    snapshot(&state)
}

#[tauri::command]
pub(crate) async fn sync_subtitle_presentation(app: AppHandle) -> Result<(), String> {
    let running = {
        let state = app.state::<RuntimeState>();
        let phase = state
            .subtitle_runtime
            .session
            .lock()
            .map_err(|_| "字幕状态锁失败")?
            .phase;
        !matches!(phase, SubtitlePhase::Idle)
    };
    if running {
        reload_prefs_and_render(&app)?;
    }
    Ok(())
}

#[tauri::command]
pub(crate) fn apply_subtitle_obs_routing(app: AppHandle) -> Result<(), String> {
    reload_prefs_and_render(&app)
}

pub(crate) fn domain_snapshot(state: &RuntimeState) -> Result<DomainSnapshot, String> {
    let session = state
        .subtitle_runtime
        .session
        .lock()
        .map_err(|_| "字幕状态锁失败")?;
    Ok(DomainSnapshot {
        state: match session.phase {
            SubtitlePhase::Idle => DomainRunState::Idle,
            SubtitlePhase::Stopping => DomainRunState::Stopping,
            SubtitlePhase::Failed => DomainRunState::Failed,
            _ => DomainRunState::Running,
        },
        session_id: session.public_id.clone(),
    })
}

async fn toggle(app: AppHandle) -> Result<(), String> {
    let state = app.state::<RuntimeState>();
    let operation = state.subtitle_runtime.operation.clone();
    let _guard = operation.lock().await;
    let phase = state
        .subtitle_runtime
        .session
        .lock()
        .map_err(|_| "字幕状态锁失败")?
        .phase;
    if phase == SubtitlePhase::Failed {
        stop_locked(app.clone()).await?;
        start(app).await
    } else if phase == SubtitlePhase::Idle {
        start(app).await
    } else {
        stop_locked(app).await
    }
}

async fn start(app: AppHandle) -> Result<(), String> {
    let state = app.state::<RuntimeState>();
    let (prefs, audio_prefs) = read_prefs(&state)?;
    let epoch = state.subtitle_runtime.epochs.fetch_add(1, Ordering::AcqRel) + 1;
    let (source, device) = prefs.source();
    let lease = state.audio_session.acquire(AudioOwner::Subtitles)?;
    state.audio_session.attach(&lease, "subtitles")?;
    let audio = match source {
        SourceKind::Mic => start_backend_mic_inner(device, &state),
        SourceKind::System => start_backend_system_audio_inner(device, &state),
    };
    let audio = match audio {
        Ok(value) => value,
        Err(error) => {
            let _ = state.audio_session.release(&lease);
            return Err(error);
        }
    };
    {
        let mut session = state
            .subtitle_runtime
            .session
            .lock()
            .map_err(|_| "字幕状态锁失败")?;
        *session = Session {
            epoch,
            public_id: Some(Uuid::new_v4().to_string()),
            phase: if audio_prefs.subtitle_silence_disconnect_enabled {
                SubtitlePhase::WaitingForVoice
            } else {
                SubtitlePhase::Running
            },
            source,
            sample_rate: audio.sample_rate,
            lease: Some(lease),
            prefs,
            audio_prefs,
            ..Session::default()
        };
    }
    let raw_rx = match match source {
        SourceKind::Mic => attach_backend_mic_raw_inner(&state),
        SourceKind::System => attach_backend_system_audio_raw_inner(&state),
    } {
        Ok((_, receiver)) => receiver,
        Err(error) => {
            cleanup_start_failure(&state, source);
            return Err(error);
        }
    };
    spawn_raw_consumer(app.clone(), epoch, raw_rx);
    let should_open = !state
        .subtitle_runtime
        .session
        .lock()
        .map_err(|_| "字幕状态锁失败")?
        .audio_prefs
        .subtitle_silence_disconnect_enabled;
    if should_open {
        if let Err(error) = open_asr(app.clone(), epoch).await {
            cleanup_start_failure(&state, source);
            return Err(error);
        }
    }
    if let Err(error) = sync_presentation(&app) {
        cleanup_start_failure(&state, source);
        return Err(error);
    }
    schedule_obs_layout(app.clone());
    publish_state(&app);
    spawn_obs_monitor(app, epoch);
    Ok(())
}

fn cleanup_start_failure(state: &RuntimeState, source: SourceKind) {
    let (asr, lease) = state
        .subtitle_runtime
        .session
        .lock()
        .ok()
        .map(|mut session| (session.asr_session_id.take(), session.lease.take()))
        .unwrap_or_default();
    if let Some(id) = asr {
        let _ = stop_asr_stream_inner(&id, state);
    }
    match source {
        SourceKind::Mic => {
            let _ = release_backend_mic_inner(state);
        }
        SourceKind::System => {
            let _ = release_backend_system_audio_inner(state);
        }
    }
    if let Some(lease) = lease {
        let _ = state.audio_session.release(&lease);
    }
    if let Ok(mut session) = state.subtitle_runtime.session.lock() {
        *session = Session::default();
    }
}

async fn stop(app: AppHandle) -> Result<(), String> {
    let state = app.state::<RuntimeState>();
    let operation = state.subtitle_runtime.operation.clone();
    let _guard = operation.lock().await;
    stop_locked(app).await
}

async fn stop_locked(app: AppHandle) -> Result<(), String> {
    let state = app.state::<RuntimeState>();
    let (source, asr, lease) = {
        let mut session = state
            .subtitle_runtime
            .session
            .lock()
            .map_err(|_| "字幕状态锁失败")?;
        if session.phase == SubtitlePhase::Idle {
            return Ok(());
        }
        session.phase = SubtitlePhase::Stopping;
        (
            session.source,
            session.asr_session_id.take(),
            session.lease.take(),
        )
    };
    publish_state(&app);
    if let Some(id) = asr {
        let _ = stop_asr_stream_inner(&id, &state);
    }
    if lease.is_some() {
        match source {
            SourceKind::Mic => {
                let _ = pause_backend_mic_inner(&state);
                let _ = release_backend_mic_inner(&state);
            }
            SourceKind::System => {
                let _ = pause_backend_system_audio_inner(&state);
                let _ = release_backend_system_audio_inner(&state);
            }
        }
    }
    if let Some(lease) = lease {
        let _ = state.audio_session.release(&lease);
    }
    {
        let mut session = state
            .subtitle_runtime
            .session
            .lock()
            .map_err(|_| "字幕状态锁失败")?;
        *session = Session::default();
    }
    clear_outputs(&app);
    publish_state(&app);
    Ok(())
}

fn spawn_raw_consumer(
    app: AppHandle,
    epoch: u64,
    mut rx: tokio::sync::mpsc::UnboundedReceiver<AsrStreamInput>,
) {
    tauri::async_runtime::spawn(async move {
        while let Some(input) = rx.recv().await {
            let AsrStreamInput::RawF32(samples) = input else {
                continue;
            };
            let level = rms(&samples);
            let (open, close) = {
                let state = app.state::<RuntimeState>();
                let Ok(mut session) = state.subtitle_runtime.session.lock() else {
                    break;
                };
                if session.epoch != epoch
                    || matches!(
                        session.phase,
                        SubtitlePhase::Idle | SubtitlePhase::Stopping | SubtitlePhase::Failed
                    )
                {
                    break;
                }
                if !session.audio_prefs.subtitle_silence_disconnect_enabled {
                    (false, false)
                } else if level > session.audio_prefs.subtitle_silence_threshold {
                    session.last_voice_at = Some(Instant::now());
                    let open = session.asr_session_id.is_none() && !session.opening;
                    if open {
                        session.opening = true;
                    }
                    (open, false)
                } else {
                    let close = session.asr_session_id.is_some()
                        && session.last_voice_at.is_some_and(|at| {
                            at.elapsed()
                                >= Duration::from_millis(
                                    session.audio_prefs.subtitle_silence_disconnect_ms,
                                )
                        });
                    (false, close)
                }
            };
            if close {
                disconnect_for_silence(&app, epoch);
            }
            if open {
                if let Err(error) = open_asr(app.clone(), epoch).await {
                    fail_and_cleanup(app.clone(), error).await;
                }
                if let Ok(mut session) = app.state::<RuntimeState>().subtitle_runtime.session.lock()
                {
                    if session.epoch == epoch {
                        session.opening = false;
                    }
                }
            }
        }
    });
}

async fn open_asr(app: AppHandle, epoch: u64) -> Result<(), String> {
    let state = app.state::<RuntimeState>();
    let (model, rate, dsp, source) = {
        let session = state
            .subtitle_runtime
            .session
            .lock()
            .map_err(|_| "字幕状态锁失败")?;
        if session.epoch != epoch || session.asr_session_id.is_some() {
            return Ok(());
        }
        (
            session.prefs.asr_model.clone(),
            session.sample_rate,
            session.audio_prefs.dsp.clone(),
            session.source,
        )
    };
    let response = start_asr_stream_inner(
        app.clone(),
        &state,
        None,
        Some(model),
        Some(rate),
        Some(dsp),
    )
    .await?;
    let attached = match source {
        SourceKind::Mic => attach_backend_mic_to_asr_inner(&response.session_id, &state),
        SourceKind::System => {
            attach_backend_system_audio_to_asr_inner(&response.session_id, &state)
        }
    };
    if let Err(error) = attached {
        let _ = stop_asr_stream_inner(&response.session_id, &state);
        return Err(error);
    }
    let mut session = state
        .subtitle_runtime
        .session
        .lock()
        .map_err(|_| "字幕状态锁失败")?;
    if session.epoch != epoch {
        drop(session);
        let _ = stop_asr_stream_inner(&response.session_id, &state);
        return Ok(());
    }
    session.asr_session_id = Some(response.session_id);
    session.phase = SubtitlePhase::Running;
    session.reconnect_attempts = 0;
    drop(session);
    publish_state(&app);
    Ok(())
}

fn disconnect_for_silence(app: &AppHandle, epoch: u64) {
    let state = app.state::<RuntimeState>();
    let id = state
        .subtitle_runtime
        .session
        .lock()
        .ok()
        .and_then(|mut session| {
            if session.epoch != epoch {
                return None;
            }
            session.phase = SubtitlePhase::WaitingForVoice;
            session.last_voice_at = None;
            session.asr_session_id.take()
        });
    if let Some(id) = id {
        let _ = stop_asr_stream_inner(&id, &state);
    }
    publish_state(app);
}

async fn handle_backend_event(app: AppHandle, event: BackendEvent) {
    match event {
        BackendEvent::Asr {
            session_id,
            kind,
            payload,
        } => handle_asr(app, session_id, kind, payload).await,
        BackendEvent::SubtitleTranslation {
            epoch,
            segment_seq,
            text,
            done,
            error,
        } => {
            let _completed = done;
            handle_translation(&app, epoch, segment_seq, text, error)
        }
        BackendEvent::Transcription { .. } => {}
    }
}

async fn handle_asr(app: AppHandle, session_id: String, kind: String, payload: Value) {
    let mut translate = vec![];
    let mut reconnect = None;
    {
        let state = app.state::<RuntimeState>();
        let Ok(mut session) = state.subtitle_runtime.session.lock() else {
            return;
        };
        if session.asr_session_id.as_deref() != Some(&session_id) {
            return;
        }
        match kind.as_str() {
            "result" => {
                if let Some(text) = payload.get("text").and_then(Value::as_str) {
                    let final_result = payload.get("final").and_then(Value::as_bool) == Some(true);
                    let now = Instant::now();
                    let mode = session.prefs.mode.clone();
                    let continuing = !session.document.replace_line.is_empty()
                        && session
                            .document
                            .replace_line_at
                            .is_some_and(|at| now.duration_since(at) <= REPLACE_CONTINUE_GAP);
                    session.document.on_partial(text.to_string(), &mode, now);
                    if session.prefs.translation_enabled() {
                        translate = session.translation.dispatch(text, final_result);
                    }
                    if final_result {
                        session.document.commit(&mode, now);
                        session.translation.commit(&mode, continuing);
                    }
                }
            }
            "ended" | "closed" | "error" => {
                session.asr_session_id = None;
                if session.audio_prefs.subtitle_silence_disconnect_enabled {
                    session.phase = SubtitlePhase::WaitingForVoice;
                } else {
                    session.reconnect_attempts += 1;
                    session.phase = SubtitlePhase::Reconnecting;
                    reconnect = Some((session.epoch, session.reconnect_attempts));
                }
            }
            _ => {}
        }
    }
    render(&app);
    for (seq, text) in translate {
        spawn_translation(app.clone(), seq, text);
    }
    if let Some((epoch, attempt)) = reconnect {
        spawn_reconnect(app, epoch, attempt);
    }
}

fn spawn_reconnect(app: AppHandle, epoch: u64, attempt: u32) {
    tauri::async_runtime::spawn(async move {
        if attempt > MAX_RECONNECT_ATTEMPTS {
            fail_and_cleanup(app.clone(), "字幕 ASR 连接反复中断".into()).await;
            return;
        }
        tokio::time::sleep(Duration::from_millis((300 * attempt as u64).min(2_000))).await;
        let current = app
            .state::<RuntimeState>()
            .subtitle_runtime
            .session
            .lock()
            .map(|s| s.epoch == epoch && s.phase == SubtitlePhase::Reconnecting)
            .unwrap_or(false);
        if current {
            if let Err(error) = open_asr(app.clone(), epoch).await {
                fail_and_cleanup(app.clone(), error).await;
            }
        }
    });
}

fn spawn_translation(app: AppHandle, segment_seq: u64, text: String) {
    let epoch = app
        .state::<RuntimeState>()
        .subtitle_runtime
        .session
        .lock()
        .map(|session| session.epoch)
        .unwrap_or(0);
    let prepared = (|| -> Result<_, String> {
        let state = app.state::<RuntimeState>();
        let session = state
            .subtitle_runtime
            .session
            .lock()
            .map_err(|_| "字幕状态锁失败")?;
        let model = session.prefs.translation_model.clone();
        let plugin_provider = state
            .plugin_registry
            .lock()
            .map_err(|_| "插件注册表锁失败")?
            .provider_id_for_model(&model);
        let provider_id = resolve_provider_id(&state, "translation", plugin_provider)?;
        let settings = read_provider_settings(&state)?;
        let profile = find_profile(&settings, &provider_id)
            .ok_or_else(|| format!("供应商 {provider_id} 不存在"))?;
        let plugin = state
            .plugin_registry
            .lock()
            .map_err(|_| "插件注册表锁失败")?
            .process_for_provider(&provider_id)?;
        let provider =
            translation_for_with_plugin(profile, plugin).map_err(|error| error.to_string())?;
        Ok((
            model,
            session.prefs.translation_source_lang.clone(),
            session.prefs.translation_target_lang.clone(),
            provider,
        ))
    })();
    let (model, source_lang, target_lang, provider) = match prepared {
        Ok(value) => value,
        Err(error) => {
            app.state::<RuntimeState>()
                .backend_events
                .publish(BackendEvent::SubtitleTranslation {
                    epoch,
                    segment_seq,
                    text: String::new(),
                    done: true,
                    error: Some(error),
                });
            return;
        }
    };
    tauri::async_runtime::spawn(async move {
        let hub = app.state::<RuntimeState>().backend_events.sender_clone();
        let delta_hub = hub.clone();
        let result = provider
            .translate_streaming(&model, &text, &source_lang, &target_lang, move |partial| {
                let _ = delta_hub.send(BackendEvent::SubtitleTranslation {
                    epoch,
                    segment_seq,
                    text: partial.into(),
                    done: false,
                    error: None,
                });
            })
            .await;
        let event = match result {
            Ok(text) => BackendEvent::SubtitleTranslation {
                epoch,
                segment_seq,
                text,
                done: true,
                error: None,
            },
            Err(error) => BackendEvent::SubtitleTranslation {
                epoch,
                segment_seq,
                text: String::new(),
                done: true,
                error: Some(error),
            },
        };
        let _ = hub.send(event);
    });
}

fn handle_translation(app: &AppHandle, epoch: u64, seq: u64, text: String, error: Option<String>) {
    let state = app.state::<RuntimeState>();
    let Ok(mut session) = state.subtitle_runtime.session.lock() else {
        return;
    };
    if session.epoch != epoch
        || matches!(session.phase, SubtitlePhase::Idle | SubtitlePhase::Stopping)
    {
        return;
    }
    if let Some(error) = error {
        session.error = Some(format!("字幕翻译失败：{error}"));
    } else {
        session.apply_translation(epoch, seq, text);
    }
    drop(session);
    render(app);
}

fn reload_prefs_and_render(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<RuntimeState>();
    let prefs = read_subtitle_prefs(&state)?;
    {
        let mut session = state
            .subtitle_runtime
            .session
            .lock()
            .map_err(|_| "字幕状态锁失败")?;
        if matches!(session.phase, SubtitlePhase::Idle | SubtitlePhase::Stopping) {
            return Ok(());
        }
        session.prefs = prefs;
    }
    sync_presentation(app)?;
    schedule_obs_layout(app.clone());
    render(app);
    Ok(())
}

fn read_prefs(state: &RuntimeState) -> Result<(SubtitlePrefs, AudioPrefs), String> {
    let settings = state.app_settings.lock().map_err(|_| "应用配置锁失败")?;
    let subtitles = serde_json::from_value(settings.subtitle_prefs.clone())
        .map_err(|e| format!("字幕配置无效：{e}"))?;
    let audio = serde_json::from_value(settings.dictation_prefs.clone())
        .map_err(|e| format!("音频配置无效：{e}"))?;
    Ok((subtitles, audio))
}

fn read_subtitle_prefs(state: &RuntimeState) -> Result<SubtitlePrefs, String> {
    serde_json::from_value(
        state
            .app_settings
            .lock()
            .map_err(|_| "应用配置锁失败")?
            .subtitle_prefs
            .clone(),
    )
    .map_err(|e| format!("字幕配置无效：{e}"))
}

fn render(app: &AppHandle) {
    let state = app.state::<RuntimeState>();
    let status = overlay_status(&state).ok();
    let raw_obs_active = status.is_some_and(|v| v.ready && v.connected);
    let (original, translation, prefs, active_before, obs_active) = {
        let Ok(mut session) = state.subtitle_runtime.session.lock() else {
            return;
        };
        let active_before = session.obs_active;
        let obs_active = if !session.prefs.obs_output_enabled {
            session.obs_disconnected_at = None;
            false
        } else if raw_obs_active {
            session.obs_disconnected_at = None;
            true
        } else if session.obs_active {
            let disconnected_at = *session.obs_disconnected_at.get_or_insert_with(Instant::now);
            disconnected_at.elapsed() < Duration::from_secs(2)
        } else {
            false
        };
        session.obs_active = obs_active;
        (
            session.document.display(&session.prefs),
            session.translation.display(&session.prefs),
            session.prefs.clone(),
            active_before,
            obs_active,
        )
    };
    let style = overlay_style(&prefs);
    publish_overlay_snapshot(
        &state,
        ObsOverlaySnapshot {
            original_text: if prefs.obs_output_enabled {
                original.clone()
            } else {
                String::new()
            },
            translation_text: if prefs.obs_output_enabled {
                translation.clone()
            } else {
                String::new()
            },
            style,
        },
    );
    let (main, secondary) = if !prefs.translation_enabled() {
        (original.clone(), String::new())
    } else if prefs.translation_layout == "translationOnly" {
        (translation.clone(), String::new())
    } else {
        (original.clone(), translation.clone())
    };
    let _ = crate::desktop::set_indicator_text(app.clone(), main, None);
    let _ = crate::desktop::set_indicator_translation(app.clone(), secondary);
    if active_before != obs_active {
        let _ = crate::desktop::set_indicator_state(
            app.clone(),
            if obs_active {
                "hidden".into()
            } else {
                "subtitle".into()
            },
        );
    }
    publish_state(app);
}

fn sync_presentation(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<RuntimeState>();
    let prefs = state
        .subtitle_runtime
        .session
        .lock()
        .map_err(|_| "字幕状态锁失败")?
        .prefs
        .clone();
    let window = crate::desktop::ensure_indicator_window(app)?;
    let scale = window.scale_factor().unwrap_or(1.0);
    let (monitor_width, monitor_height) = window
        .current_monitor()
        .ok()
        .flatten()
        .map(|m| {
            let size = m.size();
            (size.width as f64 / scale, size.height as f64 / scale)
        })
        .unwrap_or((1920.0, 1080.0));
    let font_size = (monitor_height * prefs.font_size_percent / 100.0).round();
    let width = (monitor_width * prefs.width_percent / 100.0).round();
    let offset_y = (monitor_height * prefs.offset_y_percent / 100.0).round();
    let lines = if prefs.mode == "replace" {
        1
    } else {
        prefs.line_count.max(1)
    };
    let line_height = (font_size * 1.38).round();
    let extra = if prefs.translation_enabled() && prefs.translation_layout == "bilingual" {
        line_height * lines as f64 + 30.0
    } else {
        0.0
    };
    let height = line_height * lines as f64 + extra + 28.0;
    crate::desktop::set_indicator_layout(
        app.clone(),
        Some(width),
        Some(height),
        Some(prefs.anchor.clone()),
        Some(offset_y),
    )?;
    let _ = window.emit("dictation-indicator-config", json!({
        "mode": "subtitle",
        "subtitle": {
            "displayMode": prefs.mode, "fontFamily": prefs.font_family, "fontSize": font_size,
            "lineCount": lines, "textColor": prefs.text_color,
            "backgroundColor": rgba(&prefs.background_color, prefs.background_opacity),
            "rounded": prefs.rounded, "width": width, "windowWidth": width, "windowHeight": height,
            "anchor": prefs.anchor, "offsetY": offset_y,
            "motionEnabled": prefs.motion_enabled, "motionDurationMs": prefs.motion_duration_ms,
            "motionEasing": prefs.motion_easing, "fadeEnabled": prefs.fade_enabled,
            "fadeDurationMs": prefs.fade_duration_ms, "fadeEasing": prefs.fade_easing,
            "translationEnabled": prefs.translation_enabled(), "translationLayout": prefs.translation_layout,
            "translationOrder": prefs.translation_order
        }
    }));
    let obs_active = state
        .subtitle_runtime
        .session
        .lock()
        .map(|s| s.obs_active)
        .unwrap_or(false);
    crate::desktop::set_indicator_state(
        app.clone(),
        if obs_active {
            "hidden".into()
        } else {
            "subtitle".into()
        },
    )
}

fn schedule_obs_layout(app: AppHandle) {
    let prefs = app
        .state::<RuntimeState>()
        .subtitle_runtime
        .session
        .lock()
        .ok()
        .map(|s| s.prefs.clone());
    let Some(prefs) = prefs else { return };
    tauri::async_runtime::spawn(async move {
        let state = app.state::<RuntimeState>();
        let translation_enabled = prefs.translation_enabled();
        let _ = sync_obs_overlay_layout(
            app.clone(),
            ObsOverlayLayoutRequest {
                display_mode: prefs.mode,
                width_percent: prefs.width_percent,
                font_size_percent: prefs.font_size_percent,
                line_count: prefs.line_count,
                translation_enabled,
                translation_layout: prefs.translation_layout,
            },
            state,
        )
        .await;
    });
}

fn spawn_obs_monitor(app: AppHandle, epoch: u64) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            let state = app.state::<RuntimeState>();
            let current = state
                .subtitle_runtime
                .session
                .lock()
                .map(|s| {
                    s.epoch == epoch
                        && !matches!(
                            s.phase,
                            SubtitlePhase::Idle | SubtitlePhase::Stopping | SubtitlePhase::Failed
                        )
                })
                .unwrap_or(false);
            if !current {
                break;
            }
            let raw_active = overlay_status(&state)
                .map(|v| v.ready && v.connected)
                .unwrap_or(false);
            let should_render = state
                .subtitle_runtime
                .session
                .lock()
                .map(|mut s| {
                    if raw_active && !s.obs_active {
                        return true;
                    }
                    if !raw_active && s.obs_active {
                        let disconnected_at =
                            *s.obs_disconnected_at.get_or_insert_with(Instant::now);
                        return disconnected_at.elapsed() >= Duration::from_secs(2);
                    }
                    false
                })
                .unwrap_or(false);
            if should_render {
                render(&app);
            }
        }
    });
}

fn snapshot(state: &RuntimeState) -> Result<SubtitleSnapshot, String> {
    let session = state
        .subtitle_runtime
        .session
        .lock()
        .map_err(|_| "字幕状态锁失败")?;
    Ok(SubtitleSnapshot {
        phase: session.phase,
        session_id: session.public_id.clone(),
        original_text: session.document.display(&session.prefs),
        translation_text: session.translation.display(&session.prefs),
        obs_output_active: session.obs_active,
        error: session.error.clone(),
    })
}

fn publish_state(app: &AppHandle) {
    let state = app.state::<RuntimeState>();
    let Ok(payload) =
        snapshot(&state).and_then(|v| serde_json::to_value(v).map_err(|e| e.to_string()))
    else {
        return;
    };
    let session_id = state
        .subtitle_runtime
        .session
        .lock()
        .ok()
        .and_then(|s| s.public_id.clone());
    let event = DomainEventEnvelope {
        revision: next_revision(&state.snapshot_revision),
        domain: "subtitles".into(),
        event_type: "stateChanged".into(),
        session_id,
        payload,
    };
    let _ = app.emit(DOMAIN_EVENT, event);
}

fn fail(app: &AppHandle, error: String) {
    if let Ok(mut session) = app.state::<RuntimeState>().subtitle_runtime.session.lock() {
        session.phase = SubtitlePhase::Failed;
        session.error = Some(error);
    }
    publish_state(app);
}

async fn fail_and_cleanup(app: AppHandle, error: String) {
    let _ = stop(app.clone()).await;
    fail(&app, error);
}

fn clear_outputs(app: &AppHandle) {
    let state = app.state::<RuntimeState>();
    publish_overlay_snapshot(&state, ObsOverlaySnapshot::default());
    let _ = crate::desktop::set_indicator_text(app.clone(), String::new(), None);
    let _ = crate::desktop::set_indicator_translation(app.clone(), String::new());
    let _ = crate::desktop::set_indicator_state(app.clone(), "hidden".into());
}

fn clause_cut(text: &str) -> Option<usize> {
    let mut hard = None;
    for (index, ch) in text.char_indices() {
        let end = index + ch.len_utf8();
        if matches!(ch, '。' | '！' | '？' | '；' | '…' | '.' | '!' | '?') {
            hard = Some(end);
        }
    }
    if hard.is_some() {
        return hard;
    }
    let mut commas = 0;
    for (index, ch) in text.char_indices() {
        if matches!(ch, '，' | ',') {
            commas += 1;
            if commas == 2 {
                return Some(index + ch.len_utf8());
            }
        }
    }
    (text.chars().count() >= CLAUSE_MAX_CHARS).then_some(text.len())
}

fn tail_chars(text: &str, max: usize) -> String {
    let count = text.chars().count();
    if count <= max {
        text.to_string()
    } else {
        text.chars()
            .skip(count - max)
            .collect::<String>()
            .trim_start()
            .into()
    }
}

fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    (samples.iter().map(|v| v * v).sum::<f32>() / samples.len() as f32).sqrt()
}

fn rgba(hex: &str, opacity: f64) -> String {
    let raw = hex.trim_start_matches('#');
    let full = if raw.len() == 3 {
        raw.chars().flat_map(|c| [c, c]).collect::<String>()
    } else {
        format!("{raw:0<6}").chars().take(6).collect()
    };
    let value = u32::from_str_radix(&full, 16).unwrap_or(0);
    format!(
        "rgba({}, {}, {}, {})",
        (value >> 16) & 255,
        (value >> 8) & 255,
        value & 255,
        (opacity / 100.0).clamp(0.0, 1.0)
    )
}

fn overlay_style(prefs: &SubtitlePrefs) -> ObsOverlayStyle {
    ObsOverlayStyle {
        display_mode: prefs.mode.clone(),
        font_family: prefs.font_family.clone(),
        font_size: (1080.0 * prefs.font_size_percent / 100.0).round() as u32,
        font_size_percent: prefs.font_size_percent,
        line_count: if prefs.mode == "replace" {
            1
        } else {
            prefs.line_count
        },
        width_percent: prefs.width_percent,
        text_color: prefs.text_color.clone(),
        background_color: rgba(&prefs.background_color, prefs.background_opacity),
        rounded: prefs.rounded,
        motion_enabled: prefs.motion_enabled,
        motion_duration_ms: prefs.motion_duration_ms,
        motion_easing: prefs.motion_easing.clone(),
        fade_enabled: prefs.fade_enabled,
        fade_duration_ms: prefs.fade_duration_ms,
        fade_easing: prefs.fade_easing.clone(),
        translation_enabled: prefs.translation_enabled(),
        translation_layout: prefs.translation_layout.clone(),
        translation_order: prefs.translation_order.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replacement_continues_within_gap_and_resets_after_gap() {
        let now = Instant::now();
        let mut doc = SubtitleDocument::default();
        doc.on_partial("第一句".into(), "replace", now);
        doc.commit("replace", now);
        doc.on_partial("第二句".into(), "replace", now + Duration::from_secs(2));
        doc.commit("replace", now + Duration::from_secs(2));
        assert_eq!(doc.replace_line, "第一句 第二句");
        doc.on_partial("新行".into(), "replace", now + Duration::from_secs(5));
        assert_eq!(doc.display(&SubtitlePrefs::default()), "新行");
    }

    #[test]
    fn scroll_mode_crops_to_visible_lines() {
        let mut doc = SubtitleDocument::default();
        for line in ["一", "二", "三"] {
            doc.on_partial(line.into(), "scroll", Instant::now());
            doc.commit("scroll", Instant::now());
        }
        let prefs = SubtitlePrefs {
            mode: "scroll".into(),
            line_count: 2,
            ..SubtitlePrefs::default()
        };
        assert_eq!(doc.display(&prefs), "二\n三");
    }

    #[test]
    fn clause_split_prefers_punctuation_and_forces_long_tail() {
        assert_eq!(clause_cut("你好，世界，再见"), Some("你好，世界，".len()));
        assert_eq!(
            clause_cut("这是完整句子。后续"),
            Some("这是完整句子。".len())
        );
        assert!(clause_cut(&"字".repeat(60)).is_some());
    }

    #[test]
    fn translation_order_is_rebuilt_by_sequence_not_arrival() {
        let mut doc = TranslationDocument::default();
        doc.current_group = vec![1, 2];
        doc.update(2, "world".into());
        doc.update(1, "hello ".into());
        assert_eq!(doc.display(&SubtitlePrefs::default()), "hello world");
    }

    #[test]
    fn late_translation_epoch_is_rejected_by_session_guard() {
        let mut session = Session {
            epoch: 8,
            phase: SubtitlePhase::Running,
            ..Session::default()
        };
        assert!(!session.apply_translation(7, 1, "旧会话".into()));
        assert!(session.apply_translation(8, 1, "当前会话".into()));
        assert_eq!(session.translation.values.get(&1).unwrap(), "当前会话");
    }
}
