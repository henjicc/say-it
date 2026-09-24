use crate::application::audio_session::{AudioLease, AudioOwner};
use crate::application::contract::{
    next_revision, DomainEventEnvelope, DomainRunState, DomainSnapshot,
};
use crate::application::events::BackendEvent;
use crate::commands::asr::{prepare_asr_stream_inner, stop_asr_stream_inner};
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
use crate::state::{RawAudioReceiver, RuntimeState};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tauri::AppHandle;
use tokio_util::sync::CancellationToken;

mod preview;
mod retention;
mod translation_queue;
mod translation_work;
#[cfg(test)]
mod delivery_tests;
pub(crate) use preview::{hide_subtitle_preview, show_subtitle_preview};

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
pub(crate) struct SubtitlePrefs {
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
            font_family: default_subtitle_font_family(),
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

fn default_subtitle_font_family() -> String {
    if cfg!(target_os = "macos") {
        "PingFang SC".into()
    } else {
        "Microsoft YaHei".into()
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
    /// 当前这句在单句替换模式下是否续接上一句。
    ///
    /// 判定只能在**新一句的第一个 partial** 到达时做——那一刻 `replace_line_at`
    /// 还是上一句 commit 的时刻，量到的才是真正的句间停顿。译文侧原本在 final
    /// 那一刻拿同一个陈旧时间戳重算，于是只要这句话自身说满 2.5 秒，判定就会翻成
    /// false，`TranslationDocument::commit` 走 `replace_groups = vec![group]` 把历史
    /// 译文整批丢掉；而 `SubtitleDocument::commit` 是无条件追加的。画面上原文累积成
    /// 「A B C」、译文却只剩最后一句，两行完全对不上。原文与译文必须共用这一个结论。
    replace_continuing: bool,
}

impl SubtitleDocument {
    fn on_partial(&mut self, text: String, mode: &str, now: Instant) {
        if self.current.is_empty() && mode == "replace" {
            if self
                .replace_line_at
                .is_some_and(|at| now.duration_since(at) > REPLACE_CONTINUE_GAP)
            {
                self.replace_line.clear();
            }
            // 清理之后还剩内容，说明这句接在上一句后面；译文要沿用同一结论。
            self.replace_continuing = !self.replace_line.is_empty();
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
    completed: BTreeSet<u64>,
}

impl TranslationDocument {
    /// ASR 会话断开时调用：清掉只对该会话有意义的分句游标。
    ///
    /// 不清的话，上一段话的字节游标会被带进重连后新会话的第一个 partial，
    /// 轻则吞掉新句开头的若干字节（译文缺句首），重则落在多字节字符中间导致
    /// `dispatch` 切片 panic。
    fn reset_partial(&mut self) {
        self.partial_offset = 0;
    }

    fn dispatch(&mut self, text: &str, final_result: bool) -> Vec<(u64, String)> {
        let mut out = vec![];
        // `partial_offset` 是**字节**游标。它只在 `commit()` 里归零，而 ASR 会话中断
        // （"ended"/"closed"/"error"、静音断开）不会 commit，于是上一段话的游标会被
        // 带进重连后新会话的第一个 partial。此时它既可能越界，也可能落在新文本某个
        // 多字节字符的中间——后者会让下面的切片直接 panic。而 dispatch 是在持有
        // `subtitle_runtime.session` 守卫时调用的，一次 panic 会 poison 那把锁，
        // 此后 start/stop/toggle/snapshot 全部返回「字幕状态锁失败」，音频租约也
        // 永不释放，只能重启应用。两种情况一律归零重新开始。
        if self.partial_offset > text.len() || !text.is_char_boundary(self.partial_offset) {
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
                self.values.insert(self.next_seq, String::new());
                out.push((self.next_seq, clause.to_string()));
            }
        }
        if final_result {
            self.partial_offset = text.len();
            let rest = tail.trim();
            if !rest.is_empty() {
                self.next_seq += 1;
                self.current_group.push(self.next_seq);
                self.values.insert(self.next_seq, String::new());
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
        self.prune_completed();
    }

    fn update(&mut self, seq: u64, text: &str) -> bool {
        if self.completed.contains(&seq) {
            return false;
        }
        let Some(value) = self.values.get_mut(&seq) else { return false };
        // 多保留一个字符，用来区分恰好填满与发生截断，保留原有 trim_start 语义。
        let start = text.char_indices().rev().nth(MAX_TEXT_CHARS).map_or(0, |(i, _)| i);
        *value = text[start..].to_string();
        true
    }

    fn display(&self, prefs: &SubtitlePrefs) -> String {
        self.render_tail(prefs)
    }

    #[cfg(test)]
    fn display_legacy(&self, prefs: &SubtitlePrefs) -> String {
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
    translation_cancellation: CancellationToken,
    translation_jobs: translation_queue::Queue<translation_work::Job>,
    last_voice_at: Option<Instant>,
    reconnect_attempts: u32,
    opening: bool,
    obs_active: bool,
    /// 翻译失败（非致命）。与 `error` 分开：`error` 代表整个字幕会话失败并伴随
    /// `phase = Failed`，而翻译失败时字幕本身仍在正常滚动，只是译文出不来。
    /// 混在一起会导致前端永远看不到它——`applyRuntime` 只在 phase 为 failed 时
    /// 才展示 error，其余情况一律覆盖成「实时字幕已开启」。
    translation_error: Option<String>,
    obs_disconnected_at: Option<Instant>,
    error: Option<String>,
}

impl Session {
    fn wants_asr(&self, epoch: u64) -> bool {
        self.epoch == epoch
            && matches!(
                self.phase,
                SubtitlePhase::Running | SubtitlePhase::WaitingForVoice | SubtitlePhase::Reconnecting
            )
            && self.asr_session_id.is_none()
    }

    fn record_translation(
        &mut self,
        epoch: u64,
        seq: u64,
        text: &str,
        done: bool,
        error: Option<&str>,
    ) -> bool {
        if self.epoch != epoch
            || matches!(self.phase, SubtitlePhase::Idle | SubtitlePhase::Stopping)
            || !self.translation.values.contains_key(&seq)
            || self.translation.completed.contains(&seq)
        {
            return false;
        }
        if let Some(error) = error {
            self.translation_error = Some(format!("字幕翻译失败：{error}"));
            if done {
                self.translation.finish(seq);
            }
        } else {
            self.translation_error = None;
            self.apply_translation(epoch, seq, text, done);
        }
        self.cancel_obsolete_translations();
        true
    }

    fn cancel_obsolete_translations(&mut self) {
        let enabled = self.prefs.translation_enabled();
        if !enabled {
            // 这些请求已被取消，不会再发出 done；保留已有 partial，并释放空占位。
            self.translation.completed.extend(self.translation.values.keys().copied());
            self.translation.prune_completed();
        }
        self.translation_jobs.retain(|seq| enabled && self.translation.values.contains_key(&seq));
    }

    fn apply_translation(&mut self, epoch: u64, seq: u64, text: &str, done: bool) -> bool {
        if self.epoch != epoch
            || matches!(self.phase, SubtitlePhase::Idle | SubtitlePhase::Stopping)
        {
            return false;
        }
        let accepted = self.translation.update(seq, text);
        if accepted && done {
            self.translation.finish(seq);
        }
        accepted
    }
}

pub(crate) struct SubtitleRuntime {
    session: Arc<Mutex<Session>>,
    preview: Mutex<Option<preview::Preview>>,
    operation: Arc<tokio::sync::Mutex<()>>,
    epochs: AtomicU64,
    translation_changed: Arc<tokio::sync::Notify>,
}

impl Default for SubtitleRuntime {
    fn default() -> Self {
        Self {
            session: Arc::new(Mutex::new(Session::default())),
            preview: Mutex::new(None),
            operation: Arc::new(tokio::sync::Mutex::new(())),
            epochs: AtomicU64::new(0),
            translation_changed: Arc::new(tokio::sync::Notify::new()),
        }
    }
}

impl SubtitleRuntime {
    fn record_translation(
        &self,
        epoch: u64,
        seq: u64,
        text: &str,
        done: bool,
        error: Option<&str>,
    ) -> Result<bool, String> {
        let mut session = self.session.lock().map_err(|_| "字幕状态锁失败")?;
        if !session.record_translation(epoch, seq, text, done, error) {
            return Ok(false);
        }
        drop(session);
        // 状态先提交，通知只表示“有更新”；慢渲染合并通知，不丢失最终状态或累积载荷。
        self.translation_changed.notify_one();
        Ok(true)
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SubtitleSnapshot {
    phase: SubtitlePhase,
    preview_active: bool,
    session_id: Option<String>,
    original_text: String,
    translation_text: String,
    obs_output_active: bool,
    error: Option<String>,
    translation_error: Option<String>,
}

pub(crate) fn initialize(app: AppHandle) {
    let mut receiver = app.state::<RuntimeState>().backend_events.subscribe();
    let translation_changed = app.state::<RuntimeState>().subtitle_runtime.translation_changed.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::select! {
                _ = translation_changed.notified() => render(&app),
                event = receiver.recv() => match event {
                    Ok(event) => handle_backend_event(app.clone(), event).await,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(count)) => {
                        dlog!("[subtitles] 后端事件积压，跳过 {count} 条")
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
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
pub(crate) async fn sync_subtitle_presentation(
    app: AppHandle,
    rehydrate: Option<bool>,
    preview_prefs: Option<SubtitlePrefs>,
) -> Result<(), String> {
    let state = app.state::<RuntimeState>();
    let _guard = state.subtitle_runtime.operation.lock().await;
    if rehydrate == Some(true) && !crate::desktop::indicator::can_rehydrate_subtitle_webview() {
        return Ok(());
    }
    if preview::is_active(&state) {
        return preview::refresh(&app, preview_prefs);
    }
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

/// 实时字幕是否正在占用共享的指示窗口。
///
/// 听写提示条与字幕条是**同一个**指示窗，任何延时执行的「隐藏指示窗」都必须先问一下
/// 字幕这边，否则会把用户刚开起来的字幕条一并关掉。
pub(crate) fn owns_indicator(state: &RuntimeState) -> bool {
    preview::is_active(state)
        || state
            .subtitle_runtime
            .session
            .lock()
            .map(|session| !matches!(session.phase, SubtitlePhase::Idle))
            .unwrap_or(false)
}

/// 字幕会话仍在运行且应该在屏幕上占有字幕条（OBS 接管输出时不占）。
/// 原生字幕窗用它判断「owner 空缺时文本更新要不要重新亮出字幕条」。
pub(crate) fn wants_indicator_visible(state: &RuntimeState) -> bool {
    preview::is_active(state)
        || state
            .subtitle_runtime
            .session
            .lock()
            .map(|session| !matches!(session.phase, SubtitlePhase::Idle) && !session.obs_active)
            .unwrap_or(false)
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
    preview::stop_locked(&app)?;
    let state = app.state::<RuntimeState>();
    let (prefs, audio_prefs) = read_prefs(&state)?;
    // 翻译模型的配置性错误（供应商未启用、没填 Key、插件被停用）必须在开始时就
    // 拦下来。否则字幕会正常滚动、译文永远空白，而每个 clause 都在后台静默失败
    // ——用户看不到任何线索，只会以为翻译模型不行。
    if prefs.translation_enabled() {
        crate::application::translation::validate_available(&state, &prefs.translation_model)?;
    }
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
            translation_cancellation: CancellationToken::new(),
            ..Session::default()
        };
    }
    let raw_rx = match match source {
        SourceKind::Mic => attach_backend_mic_raw_inner(&state, crate::state::AsrPreroll::Enabled),
        SourceKind::System => attach_backend_system_audio_raw_inner(&state),
    } {
        Ok((_, receiver)) => receiver,
        Err(error) => {
            cleanup_start_failure(&state, source);
            return Err(error);
        }
    };
    spawn_raw_consumer(app.clone(), epoch, source, raw_rx);
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
    let (asr, lease, translation_cancellation) = state
        .subtitle_runtime
        .session
        .lock()
        .ok()
        .map(|mut session| {
            (
                session.asr_session_id.take(),
                session.lease.take(),
                session.translation_cancellation.clone(),
            )
        })
        .unwrap_or_default();
    translation_cancellation.cancel();
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
    preview::stop_locked(&app)?;
    let state = app.state::<RuntimeState>();
    let (source, asr, lease, translation_cancellation) = {
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
            session.translation_cancellation.clone(),
        )
    };
    translation_cancellation.cancel();
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

fn spawn_raw_consumer(app: AppHandle, epoch: u64, source: SourceKind, mut rx: RawAudioReceiver) {
    tauri::async_runtime::spawn(async move {
        let mut queue_error = None;
        loop {
            let samples = match rx.recv().await {
                Ok(Some(samples)) => samples,
                Ok(None) => break,
                Err(error) => {
                    queue_error = Some(error);
                    break;
                }
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

        let state = app.state::<RuntimeState>();
        let capture_error = match source {
            SourceKind::Mic => &state.backend_mic,
            SourceKind::System => &state.backend_system_audio,
        }
        .lock()
        .ok()
        .and_then(|mut capture| capture.last_error.take());
        let still_active = state
            .subtitle_runtime
            .session
            .lock()
            .map(|session| {
                session.epoch == epoch
                    && !matches!(
                        session.phase,
                        SubtitlePhase::Idle | SubtitlePhase::Stopping | SubtitlePhase::Failed
                    )
            })
            .unwrap_or(false);
        if still_active {
            fail_and_cleanup(
                app,
                queue_error
                    .or(capture_error)
                    .unwrap_or_else(|| "音频采集已意外停止".into()),
            )
            .await;
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
        if !session.wants_asr(epoch) {
            return Ok(());
        }
        (
            session.prefs.asr_model.clone(),
            session.sample_rate,
            session.audio_prefs.dsp.clone(),
            session.source,
        )
    };
    let response = prepare_asr_stream_inner(
        app.clone(),
        &state,
        None,
        Some(model),
        Some(rate),
        Some(dsp),
    )
    .await?;
    // 准备期间可能已经停止、失败或切换会话；过期连接不得覆盖正在使用的音频路由。
    if !state.subtitle_runtime.session.lock()
        .map_err(|_| "字幕状态锁失败")?.wants_asr(epoch)
    {
        return Ok(());
    }
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
    if !session.wants_asr(epoch) {
        drop(session);
        let _ = stop_asr_stream_inner(&response.session_id, &state);
        return Ok(());
    }
    session.asr_session_id = Some(response.session_id.clone());
    session.phase = SubtitlePhase::Running;
    session.reconnect_attempts = 0;
    drop(session);
    response.start()?;
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
            session.translation.reset_partial();
            session.asr_session_id.take()
        });
    if let Some(id) = id {
        let _ = stop_asr_stream_inner(&id, &state);
    }
    publish_state(app);
}

async fn handle_backend_event(app: AppHandle, event: Arc<BackendEvent>) {
    match event.as_ref() {
        BackendEvent::Asr {
            session_id,
            kind,
            payload,
        } => handle_asr(app, session_id, kind, payload).await,
        BackendEvent::Transcription { .. } => {}
    }
}

async fn handle_asr(app: AppHandle, session_id: &str, kind: &str, payload: &Value) {
    let mut translate = vec![];
    let mut reconnect = None;
    let epoch;
    {
        let state = app.state::<RuntimeState>();
        let Ok(mut session) = state.subtitle_runtime.session.lock() else {
            return;
        };
        if session.asr_session_id.as_deref() != Some(session_id) {
            return;
        }
        epoch = session.epoch;
        match kind {
            "result" => {
                if let Some(text) = payload.get("text").and_then(Value::as_str) {
                    let final_result = payload.get("final").and_then(Value::as_bool) == Some(true);
                    let now = Instant::now();
                    let mode = session.prefs.mode.clone();
                    session.document.on_partial(text.to_string(), &mode, now);
                    if session.prefs.translation_enabled() {
                        translate = session.translation.dispatch(text, final_result);
                    }
                    if final_result {
                        // 续接与否由 `on_partial` 在本句开头就定下来，这里只是读取，
                        // 不能拿 final 时刻的 `now` 重算。
                        let continuing = session.document.replace_continuing;
                        session.document.commit(&mode, now);
                        session.translation.commit(&mode, continuing);
                    }
                }
            }
            "ended" | "closed" | "error" => {
                session.asr_session_id = None;
                session.translation.reset_partial();
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
        session.cancel_obsolete_translations();
    }
    render(&app);
    for (seq, text) in translate {
        translation_work::enqueue(app.clone(), epoch, seq, text);
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

fn handle_translation(app: &AppHandle, epoch: u64, seq: u64, text: &str, done: bool, error: Option<&str>) {
    let state = app.state::<RuntimeState>();
    if let Err(error) = state.subtitle_runtime.record_translation(epoch, seq, text, done, error) {
        crate::application::diagnostics::event("error", "subtitles.translationStateFailed", json!({"error":error}));
    }
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
        session.cancel_obsolete_translations();
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
    let (prefs, obs_active) = {
        let session = state
            .subtitle_runtime
            .session
            .lock()
            .map_err(|_| "字幕状态锁失败")?;
        (session.prefs.clone(), session.obs_active)
    };
    sync_presentation_with_prefs(app, &prefs, obs_active)
}

// 正式字幕与本地预览共用尺寸、样式和原生/WebView 路由。
fn sync_presentation_with_prefs(
    app: &AppHandle,
    prefs: &SubtitlePrefs,
    obs_active: bool,
) -> Result<(), String> {
    let native_subtitle = crate::desktop::native_subtitle::native_subtitle_enabled();
    if native_subtitle {
        crate::desktop::native_subtitle::native_subtitle_attach(app);
    }
    // 原生字幕窗固定在主显示器上，直接按主显示器尺寸换算；WebView 模式
    // 沿用指示窗当前所在显示器的测量。
    let (monitor_width, monitor_height) = if native_subtitle {
        app.primary_monitor()
            .ok()
            .flatten()
            .map(|m| {
                let scale = m.scale_factor().max(0.1);
                (
                    m.size().width as f64 / scale,
                    m.size().height as f64 / scale,
                )
            })
            .unwrap_or((1920.0, 1080.0))
    } else {
        let window = crate::desktop::ensure_indicator_window(app)?;
        let scale = window.scale_factor().unwrap_or(1.0);
        window
            .current_monitor()
            .ok()
            .flatten()
            .map(|m| {
                let size = m.size();
                (size.width as f64 / scale, size.height as f64 / scale)
            })
            .unwrap_or((1920.0, 1080.0))
    };
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
    // 原生模式下 set_indicator_layout 内部路由到原生字幕窗，不会创建 WebView。
    crate::desktop::set_indicator_layout(
        app.clone(),
        Some(width),
        Some(height),
        Some(prefs.anchor.clone()),
        Some(offset_y),
    )?;
    let subtitle_config = json!({
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
    });
    if native_subtitle {
        crate::desktop::native_subtitle::native_subtitle_set_config(subtitle_config);
    } else {
        let window = crate::desktop::ensure_indicator_window(app)?;
        let _ = window.emit("dictation-indicator-config", json!({
            "mode": "subtitle",
            "subtitle": subtitle_config
        }));
    }
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
        preview_active: preview::is_active(state),
        session_id: session.public_id.clone(),
        original_text: session.document.display(&session.prefs),
        translation_text: session.translation.display(&session.prefs),
        obs_output_active: session.obs_active,
        error: session.error.clone(),
        translation_error: session.translation_error.clone(),
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
    fn default_font_matches_the_current_desktop_platform() {
        assert_eq!(
            SubtitlePrefs::default().font_family,
            if cfg!(target_os = "macos") {
                "PingFang SC"
            } else {
                "Microsoft YaHei"
            }
        );
    }

    /// 回归：`partial_offset` 是字节游标，ASR 会话中断不会 commit，因此它会被带进
    /// 重连后新会话的第一个 partial。若该偏移落在新文本某个多字节字符中间，
    /// `&text[offset..]` 会 panic；而 dispatch 是在持有 session 锁时调用的，
    /// 一次 panic 会 poison 那把锁，整个字幕域从此不可恢复。
    #[test]
    fn dispatch_recovers_when_stale_offset_lands_inside_a_multibyte_char() {
        let mut document = TranslationDocument::default();
        // 上一句以 ASCII 标点收尾，clause_cut 把游标停在字节 4。
        let first = document.dispatch("Yes.", false);
        assert_eq!(first.len(), 1);
        assert_eq!(document.partial_offset, 4);

        // 会话中断后重连，新会话第一个 partial 是纯中文：字节 4 落在“好”(3..6) 内部。
        let text = "你好世界";
        assert!(!text.is_char_boundary(4), "用例前提：字节 4 不是字符边界");
        let out = document.dispatch(text, true);

        assert_eq!(document.partial_offset, text.len());
        assert_eq!(
            out.last().map(|(_, value)| value.as_str()),
            Some("你好世界"),
            "游标应当归零重来，整句都要派发出去，不能吞掉句首"
        );
    }

    /// 会话断开时应主动清游标，而不是依赖 dispatch 的兜底。
    #[test]
    fn reset_partial_clears_cursor_across_sessions() {
        let mut document = TranslationDocument::default();
        document.dispatch("Yes.", false);
        assert_ne!(document.partial_offset, 0);
        document.reset_partial();
        assert_eq!(document.partial_offset, 0);
    }

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

    /// 原文与译文必须用同一个续接结论。此前译文在 final 时刻用上一句 commit 的
    /// 时间戳重算，只要本句说满 `REPLACE_CONTINUE_GAP` 就会误判成「新行」，把历史
    /// 译文全部丢掉，而原文仍在累积——直播画面上两行错位。
    #[test]
    fn long_sentence_keeps_translation_in_sync_with_the_source_line() {
        let now = Instant::now();
        let mut doc = SubtitleDocument::default();
        let mut translation = TranslationDocument::default();
        let prefs = SubtitlePrefs {
            mode: "replace".into(),
            ..SubtitlePrefs::default()
        };

        // 第一句：说完立刻 final。
        doc.on_partial("第一句。".into(), "replace", now);
        for (seq, _) in translation.dispatch("第一句。", true) {
            translation.update(seq, "One.");
        }
        doc.commit("replace", now);
        translation.commit("replace", doc.replace_continuing);

        // 第二句：句间只停 0.3 秒（仍在续接窗口内），但这句本身说了 4 秒才 final。
        let start = now + Duration::from_millis(300);
        doc.on_partial("第二".into(), "replace", start);
        let finish = start + Duration::from_secs(4);
        doc.on_partial("第二句。".into(), "replace", finish);
        for (seq, _) in translation.dispatch("第二句。", true) {
            translation.update(seq, "Two.");
        }
        doc.commit("replace", finish);
        translation.commit("replace", doc.replace_continuing);

        assert_eq!(doc.display(&prefs), "第一句。 第二句。");
        assert_eq!(translation.display(&prefs), "One. Two.");
    }

    /// 句间真的停够 `REPLACE_CONTINUE_GAP` 时，原文换行、译文也必须跟着换行。
    #[test]
    fn a_real_pause_starts_a_new_line_for_both_source_and_translation() {
        let now = Instant::now();
        let mut doc = SubtitleDocument::default();
        let mut translation = TranslationDocument::default();
        let prefs = SubtitlePrefs {
            mode: "replace".into(),
            ..SubtitlePrefs::default()
        };

        doc.on_partial("第一句。".into(), "replace", now);
        for (seq, _) in translation.dispatch("第一句。", true) {
            translation.update(seq, "One.");
        }
        doc.commit("replace", now);
        translation.commit("replace", doc.replace_continuing);

        let later = now + Duration::from_secs(5);
        doc.on_partial("第二句。".into(), "replace", later);
        for (seq, _) in translation.dispatch("第二句。", true) {
            translation.update(seq, "Two.");
        }
        doc.commit("replace", later);
        translation.commit("replace", doc.replace_continuing);

        assert_eq!(doc.display(&prefs), "第二句。");
        assert_eq!(translation.display(&prefs), "Two.");
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
        doc.values.insert(1, String::new());
        doc.values.insert(2, String::new());
        doc.update(2, "world");
        doc.update(1, "hello ");
        assert_eq!(doc.display(&SubtitlePrefs::default()), "hello world");
    }

    #[test]
    fn late_translation_epoch_is_rejected_by_session_guard() {
        let mut session = Session {
            epoch: 8,
            phase: SubtitlePhase::Running,
            ..Session::default()
        };
        session.translation.dispatch("当前会话", true);
        assert!(!session.apply_translation(7, 1, "旧会话", false));
        assert!(session.apply_translation(8, 1, "当前会话", false));
        assert_eq!(session.translation.values.get(&1).unwrap(), "当前会话");
    }

    #[test]
    fn disabling_translation_settles_cancelled_placeholders_across_repeated_toggles() {
        let mut session = Session::default();
        for _ in 0..1_000 {
            session.prefs.translation_model = "test-model".into();
            session.translation.dispatch("尚未返回的分句", true);
            session.translation.commit("replace", true);
            session.prefs.translation_model = "none".into();
            session.cancel_obsolete_translations();
            assert!(session.translation.values.is_empty());
            assert!(session.translation.replace_groups.is_empty());
            assert!(session.translation.completed.is_empty());
        }
        assert!(!session.translation_cancellation.is_cancelled());
    }
}
