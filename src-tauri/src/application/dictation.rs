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
const MAX_SMART_TEMPLATES: usize = 50;
const MAX_APP_PROFILES: usize = 100;
const MAX_SMART_PROCESSING_MIN_CHARS: u32 = 10_000;

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

#[derive(Clone, Copy)]
enum CuePlaybackTarget {
    MainWindow,
    IndicatorWindow,
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

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SmartTemplate {
    id: String,
    name: String,
    prompt: String,
}

/// 按软件覆盖后处理配置。覆盖字段使用 `None` 表示继承全局，
/// 只有显式配置过的项才覆盖，否则新建一条规则会把没配的项静默关掉。
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct AppProfile {
    id: String,
    name: String,
    /// 匹配的进程名，如 `Code.exe`；大小写不敏感，语义与上下文黑名单一致。
    matchers: Vec<String>,
    enabled: bool,
    local_rules_enabled: Option<bool>,
    smart_processing_enabled: Option<bool>,
    /// `None` 跟随全局，`0` 每次听写，正数表示达到该字符数才处理。
    smart_processing_min_chars: Option<u32>,
    smart_template_id: Option<String>,
}

impl Default for AppProfile {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            matchers: vec![],
            enabled: true,
            local_rules_enabled: None,
            smart_processing_enabled: None,
            smart_processing_min_chars: None,
            smart_template_id: None,
        }
    }
}

impl AppProfile {
    fn matches(&self, identity: &crate::active_app_context::AppIdentity) -> bool {
        let process_name = identity.process_name.to_lowercase();
        let app_name = identity.app_name.to_lowercase();
        self.matchers.iter().any(|matcher| {
            let matcher = matcher.trim().to_lowercase();
            !matcher.is_empty() && (matcher == process_name || matcher == app_name)
        })
    }
}

/// 应用规则解析后实际生效的后处理配置。
#[derive(Clone, Debug)]
struct EffectivePostProcessing {
    local_rules_enabled: bool,
    smart_processing_enabled: bool,
    /// `0` 表示每次听写，正数表示达到该字符数才处理。
    smart_processing_min_chars: u32,
    smart_template_id: String,
    /// 命中的规则名，仅用于调试日志。
    matched_profile: Option<String>,
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
    smart_processing_enabled: bool,
    smart_processing_min_chars: u32,
    smart_template_id: String,
    smart_templates: Vec<SmartTemplate>,
    active_app_context_extraction_method:
        crate::active_app_context::ActiveAppContextExtractionMethod,
    active_app_context_ocr_engine: crate::active_app_context::OcrEngineKind,
    active_app_context_ocr_model: String,
    active_app_context_ocr_approved_providers: Vec<String>,
    /// 新建配置默认开启；旧配置缺失字段时必须保持原先的 eager OCR 行为。
    #[serde(default = "legacy_ocr_follow_smart_processing_min_chars")]
    active_app_context_ocr_follow_smart_processing_min_chars: bool,
    active_app_context_blocked_apps: Vec<String>,
    app_profiles_enabled: bool,
    app_profiles: Vec<AppProfile>,
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
            smart_processing_enabled: false,
            // 旧版本没有长度门槛，反序列化缺失字段时必须保持原有“每次听写”语义。
            smart_processing_min_chars: 0,
            smart_template_id: String::new(),
            smart_templates: vec![],
            active_app_context_extraction_method:
                crate::active_app_context::ActiveAppContextExtractionMethod::NativeText,
            active_app_context_ocr_engine: crate::active_app_context::OcrEngineKind::default(),
            active_app_context_ocr_model: String::new(),
            active_app_context_ocr_approved_providers: vec![],
            active_app_context_ocr_follow_smart_processing_min_chars: true,
            active_app_context_blocked_apps: vec![],
            app_profiles_enabled: false,
            app_profiles: vec![],
            mic_device_id: String::new(),
            dictation_silence_disconnect_enabled: true,
            dictation_silence_disconnect_ms: 5_000,
            dictation_silence_threshold: 0.0001,
            dsp: DspParams::default(),
        }
    }
}

fn legacy_ocr_follow_smart_processing_min_chars() -> bool {
    false
}

impl DictationPrefs {
    /// 解析当前前台软件生效的后处理配置。未启用应用规则、平台不支持、或没有命中
    /// 规则时，返回全局配置——所有失败路径都回落到「和现在一样」。
    fn resolve_post_processing(
        &self,
        identity: Option<&crate::active_app_context::AppIdentity>,
    ) -> EffectivePostProcessing {
        let global = EffectivePostProcessing {
            local_rules_enabled: self.local_rules_enabled,
            smart_processing_enabled: self.smart_processing_enabled,
            smart_processing_min_chars: self.smart_processing_min_chars,
            smart_template_id: self.smart_template_id.clone(),
            matched_profile: None,
        };
        if !self.app_profiles_enabled {
            return global;
        }
        let Some(identity) = identity else {
            return global;
        };
        // 顺序即优先级：取第一条命中的启用规则。
        let Some(profile) = self
            .app_profiles
            .iter()
            .find(|profile| profile.enabled && profile.matches(identity))
        else {
            return global;
        };
        let smart_processing_enabled = profile
            .smart_processing_enabled
            .unwrap_or(global.smart_processing_enabled);
        EffectivePostProcessing {
            local_rules_enabled: profile
                .local_rules_enabled
                .unwrap_or(global.local_rules_enabled),
            smart_processing_enabled,
            smart_processing_min_chars: profile
                .smart_processing_min_chars
                .unwrap_or(global.smart_processing_min_chars),
            // 模板只在智能处理开启时有意义；规则没指定模板就沿用全局选择。
            smart_template_id: profile
                .smart_template_id
                .clone()
                .filter(|id| !id.is_empty())
                .unwrap_or(global.smart_template_id),
            matched_profile: Some(profile.name.clone()),
        }
    }
}

fn smart_text_char_count(text: &str) -> usize {
    text.trim().chars().count()
}

fn should_run_smart_processing(effective: &EffectivePostProcessing, text: &str) -> bool {
    let char_count = smart_text_char_count(text);
    effective.smart_processing_enabled
        && char_count > 0
        && (effective.smart_processing_min_chars == 0
            || char_count >= effective.smart_processing_min_chars as usize)
}

fn should_defer_active_app_context_ocr(
    prefs: &DictationPrefs,
    effective: &EffectivePostProcessing,
) -> bool {
    prefs.active_app_context_ocr_follow_smart_processing_min_chars
        && prefs.active_app_context_extraction_method
            == crate::active_app_context::ActiveAppContextExtractionMethod::Ocr
        && effective.smart_processing_min_chars > 0
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
    /// 本次听写按前台软件解析出的后处理配置；`None` 表示尚未开始，回落全局。
    effective: Option<EffectivePostProcessing>,
    last_voice_at: Option<Instant>,
    silence_streaming: bool,
    raw_done: Option<Arc<tokio::sync::Notify>>,
    temp_audio_path: Option<PathBuf>,
    active_app_context: Option<crate::active_app_context::DictationContextCaptureHandle>,
    /// finalize 会把捕获句柄移出会话；保留独立令牌，让取消/会话替换仍能中断 OCR。
    active_app_context_cancellation: Option<crate::active_app_context::ContextCaptureCancellation>,
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
    active_app_context: Option<crate::active_app_context::ActiveAppContextSummary>,
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
    let activation_target = crate::active_app_context::activation_target();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = toggle(app.clone(), activation_target).await {
            publish_state(&app, Some(e));
        }
    });
}
pub(crate) fn request_start(app: AppHandle) {
    let activation_target = crate::active_app_context::activation_target();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = start(app.clone(), activation_target).await {
            publish_state(&app, Some(e));
        }
    });
}
pub(crate) fn request_stop(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        if let Err(e) = stop(app.clone()).await {
            // fail 已在 operation 锁内发布 Failed；此处只补充 stop 提前失败的错误。
            // 若新会话已经开始，不得把旧 stop 的错误附到新会话快照。
            let should_publish = app
                .state::<RuntimeState>()
                .dictation_runtime
                .session
                .lock()
                .map(|session| {
                    matches!(
                        session.phase,
                        DictationPhase::Finishing | DictationPhase::ProcessingFile
                    )
                })
                .unwrap_or(true);
            if should_publish {
                publish_state(&app, Some(e));
            }
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
    toggle(app, None).await
}
#[tauri::command]
pub(crate) async fn dictation_start(app: AppHandle) -> Result<(), String> {
    start(app, None).await
}
#[tauri::command]
pub(crate) async fn dictation_stop(app: AppHandle) -> Result<(), String> {
    stop(app).await
}
#[tauri::command]
pub(crate) async fn dictation_cancel(app: AppHandle) -> Result<(), String> {
    cancel(app).await
}
/// 当前可切换的软件列表，供「按软件配置规则」选择目标。仅读取窗口元信息，
/// 不读取窗口内容，与上下文黑名单无关。
#[tauri::command]
pub(crate) fn list_running_apps() -> Vec<crate::active_app_context::AppIdentity> {
    crate::active_app_context::list_running_apps()
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
    play_cue_async(
        app,
        if which == "start" { "start" } else { "end" },
        &prefs,
        CuePlaybackTarget::MainWindow,
    );
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
        active_app_context: state.active_app_context.latest_summary(),
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

async fn toggle(
    app: AppHandle,
    activation_target: Option<crate::active_app_context::ActivationTarget>,
) -> Result<(), String> {
    let phase = app
        .state::<RuntimeState>()
        .dictation_runtime
        .session
        .lock()
        .map_err(|_| "听写状态锁失败")?
        .phase;
    match phase {
        DictationPhase::Idle | DictationPhase::Failed => start(app, activation_target).await,
        DictationPhase::WaitingForVoice | DictationPhase::Recording => stop(app).await,
        _ => Ok(()),
    }
}

async fn start(
    app: AppHandle,
    activation_target: Option<crate::active_app_context::ActivationTarget>,
) -> Result<(), String> {
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
    validate_smart_processing(&prefs)?;
    let epoch = state
        .dictation_runtime
        .epochs
        .fetch_add(1, Ordering::AcqRel)
        + 1;
    // 先解析前台软件并定下生效配置，再决定是否捕获上下文正文：应用规则可能把智能
    // 处理切到含 {{active_app_context}} 的模板，顺序反了就拿不到上下文。
    let app_identity = activation_target.and_then(crate::active_app_context::app_identity);
    let effective = prefs.resolve_post_processing(app_identity.as_ref());
    if let Some(identity) = &app_identity {
        crate::development_debug_log(
            "dictation",
            format_args!(
                "前台软件={}（{}），命中规则={}，本地处理={}，智能处理={}，最少字符数={}，模板={}",
                identity.app_name,
                identity.process_name,
                effective.matched_profile.as_deref().unwrap_or("全局配置"),
                effective.local_rules_enabled,
                effective.smart_processing_enabled,
                effective.smart_processing_min_chars,
                effective.smart_template_id,
            ),
        );
    }
    let defer_active_app_context_ocr = should_defer_active_app_context_ocr(&prefs, &effective);
    let active_app_context = if effective.smart_processing_enabled {
        prefs
            .smart_templates
            .iter()
            .find(|template| template.id == effective.smart_template_id)
            .filter(|template| {
                crate::application::smart_text::requires_active_app_context(&template.prompt)
            })
            .and_then(|_| activation_target)
            .map(|target| {
                let ocr_provider = resolve_active_app_context_ocr_provider(
                    &state,
                    &prefs.active_app_context_ocr_model,
                    prefs.active_app_context_ocr_engine,
                    &prefs.active_app_context_ocr_approved_providers,
                );
                state.active_app_context.begin_dictation_capture(
                    target,
                    prefs.active_app_context_blocked_apps.clone(),
                    prefs.active_app_context_extraction_method,
                    ocr_provider,
                    defer_active_app_context_ocr,
                )
            })
    } else {
        None
    };
    let active_app_context_cancellation = active_app_context
        .as_ref()
        .map(crate::active_app_context::DictationContextCaptureHandle::cancellation);
    let info = crate::providers::registry::model_info(&prefs.asr_model)
        .cloned()
        .or_else(|| {
            state
                .plugin_registry
                .lock()
                .ok()
                .and_then(|plugins| plugins.model(&prefs.asr_model).cloned())
        })
        .ok_or_else(|| format!("听写模型未登记：{}", prefs.asr_model))?;
    let mode = if info.scenes.iter().any(|s| s == "dictationFile") {
        DictationMode::File
    } else {
        DictationMode::Realtime
    };
    if mode == DictationMode::Realtime {
        crate::application::plugin_management::refresh_browser_session_before_recording(
            &app,
            &state,
            &prefs.asr_model,
        )
        .await?;
    }
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
            effective: Some(effective),
            silence_streaming: false,
            active_app_context,
            active_app_context_cancellation,
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
    let _ = prepare_dictation_indicator(&app);
    let _ = crate::desktop::set_indicator_layout(
        app.clone(),
        Some(460.0),
        Some(188.0),
        Some("bottom".into()),
        Some(36.0),
    );
    let _ = crate::desktop::set_indicator_state(app.clone(), "recording".into());
    play_cue_async(app.clone(), "start", &prefs, CuePlaybackTarget::IndicatorWindow);
    publish_state(&app, None);
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
            if let Some(cancellation) = s.active_app_context_cancellation.take() {
                cancellation.cancel();
            }
            s.active_app_context.take();
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
        // 文件听写不建立 ASR 流，因此单独持有一份流式 DSP 只供波形预览使用。
        // 原始 PCM 仍完整保留给文件识别上传，避免预览链路改变识别输入。
        let mut waveform_dsp: Option<StreamDsp> = None;
        while let Some(input) = rx.recv().await {
            let AsrStreamInput::RawF32(samples) = input else {
                continue;
            };
            let (need_open, need_close, waveform_config) = {
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
                let waveform_config = should_show_waveform(s.mode)
                    .then(|| (s.prefs.dsp.clone(), s.sample_rate));
                (need_open, need_close, waveform_config)
            };
            if let Some((params, sample_rate)) = waveform_config {
                let dsp = waveform_dsp.get_or_insert_with(|| StreamDsp::new(params, sample_rate));
                let processed = pcm16le_to_f32(&dsp.process(&samples));
                if !processed.is_empty() {
                    let level = rms(&processed);
                    let peaks = summarize_peaks(&processed, 6);
                    emit_waveform(&app, level, peaks);
                }
            }
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
                // finalize 会短暂获取同一把 operation 锁来原子提交 UI 终态；
                // 没有 ASR 会话时这里是同步调用，必须先释放 stop 持有的锁。
                drop(_guard);
                finalize(app.clone(), epoch).await;
                return Ok(());
            }
        }
        Some(DictationMode::File) => start_file_job(app.clone(), epoch, raw, rate, prefs).await,
        None => Ok(()),
    };
    if let Err(error) = result {
        // fail 也会获取 operation 锁来保护终态 UI；先结束 stop 的临界区。
        drop(_guard);
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
    let (asr, file_job, lease, temp_path, active_app_context, active_app_context_cancellation) = {
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
            s.active_app_context.take(),
            s.active_app_context_cancellation.take(),
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
    if let Some(cancellation) = active_app_context_cancellation {
        cancellation.cancel();
    }
    drop(active_app_context);
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

fn finalize_session_is_current(
    state: &RuntimeState,
    epoch: u64,
    cancellation: Option<&crate::active_app_context::ContextCaptureCancellation>,
) -> bool {
    !cancellation.is_some_and(|cancellation| cancellation.is_cancelled())
        && state
            .dictation_runtime
            .session
            .lock()
            .map(|session| session.epoch == epoch && session.phase == DictationPhase::Injecting)
            .unwrap_or(false)
}

fn cleanup_stale_finalize(
    state: &RuntimeState,
    lease: Option<AudioLease>,
    temp_path: Option<PathBuf>,
) {
    if let Some(lease) = lease {
        let _ = state.audio_session.release(&lease);
    }
    remove_temp(temp_path);
}

async fn finalize(app: AppHandle, epoch: u64) {
    let (
        text,
        prefs,
        effective,
        method,
        mode,
        lease,
        asr,
        temp_path,
        active_app_context,
        active_app_context_cancellation,
    ) = {
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
            s.effective
                .clone()
                .unwrap_or_else(|| s.prefs.resolve_post_processing(None)),
            method,
            s.mode,
            s.lease.take(),
            s.asr_session_id.take(),
            s.temp_audio_path.take(),
            s.active_app_context.take(),
            s.active_app_context_cancellation.clone(),
        )
    };
    let should_process_smart_text = should_run_smart_processing(&effective, &text);
    if let Some(id) = asr {
        let state = app.state::<RuntimeState>();
        let _ = stop_asr_stream_inner(&id, &state);
    }
    let state = app.state::<RuntimeState>();
    {
        let operation = state.dictation_runtime.operation.clone();
        let _guard = operation.lock().await;
        if !finalize_session_is_current(&state, epoch, active_app_context_cancellation.as_ref()) {
            cleanup_stale_finalize(&state, lease, temp_path);
            return;
        }
        publish_state(&app, None);
        if should_process_smart_text {
            let _ = crate::desktop::set_indicator_state(app.clone(), "smartProcessing".into());
        }
    }
    let active_app_context = if !should_process_smart_text {
        // 延迟截图不会自行进入 OCR；尽早释放句柄与截图，短文本不占用后续资源。
        drop(active_app_context);
        if effective.smart_processing_enabled && !text.is_empty() {
            crate::development_debug_log(
                "dictation",
                format_args!(
                    "智能处理已跳过：识别文本字符数={}，最少字符数={}",
                    smart_text_char_count(&text),
                    effective.smart_processing_min_chars,
                ),
            );
        }
        String::new()
    } else if let Some(handle) = active_app_context {
        let captured = state
            .active_app_context
            .resolve_dictation_capture(handle)
            .await;
        let current_session = state.dictation_runtime.session.lock().ok();
        if current_session.as_deref().is_some_and(|session| {
            session.epoch == epoch
                && session.phase == DictationPhase::Injecting
                && !active_app_context_cancellation
                    .as_ref()
                    .is_some_and(|cancellation| cancellation.is_cancelled())
        }) {
            // 保持会话锁直到 summary 写入完成，避免取消或新会话插在
            // current 检查与 remember 之间，让旧会话覆盖最近上下文。
            state.active_app_context.remember(&captured);
            crate::development_debug_log(
                "dictation",
                format_args!(
                    "本次听写采用的软件上下文：状态={:?}，字符数={}\n--- 上下文开始 ---\n{}\n--- 上下文结束 ---",
                    captured.status,
                    captured.format_for_prompt().chars().count(),
                    captured.format_for_prompt(),
                ),
            );
            captured.format_for_prompt()
        } else {
            String::new()
        }
    } else {
        String::new()
    };
    // 延迟 OCR 最长会等待新的 5 秒截止。等待期间若用户取消或开始了新会话，
    // 旧会话不得继续调用智能处理，更不能注入文本。
    if !finalize_session_is_current(&state, epoch, active_app_context_cancellation.as_ref()) {
        cleanup_stale_finalize(&state, lease, temp_path);
        return;
    }
    // 顺序：先智能处理，再本地规则。本地规则是用户对最终文本的确定性兜底修正
    // （替换、去重、标点归一），必须作用在大模型输出之上，否则会被智能处理重新改写。
    let smart_processed = if should_process_smart_text {
        let Some(template) = prefs
            .smart_templates
            .iter()
            .find(|template| template.id == effective.smart_template_id)
        else {
            let state = app.state::<RuntimeState>();
            if let Some(lease) = &lease {
                let _ = state.audio_session.release(lease);
            }
            remove_temp(temp_path.clone());
            let _ = fail(app, epoch, "当前智能处理模板不存在".to_string()).await;
            return;
        };
        if !finalize_session_is_current(&state, epoch, active_app_context_cancellation.as_ref()) {
            cleanup_stale_finalize(&state, lease, temp_path);
            return;
        }
        let smart_result = crate::application::smart_text::process_smart_text(
            &state,
            &text,
            &template.prompt,
            &active_app_context,
        )
        .await;
        if !finalize_session_is_current(&state, epoch, active_app_context_cancellation.as_ref()) {
            cleanup_stale_finalize(&state, lease, temp_path);
            return;
        }
        match smart_result {
            Ok(value) => value,
            Err(error) => {
                if let Some(lease) = &lease {
                    let _ = state.audio_session.release(lease);
                }
                remove_temp(temp_path.clone());
                let _ = fail(app, epoch, error).await;
                return;
            }
        }
    } else {
        text
    };
    let processed = match apply_rules(&smart_processed, &prefs, effective.local_rules_enabled) {
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
    if !finalize_session_is_current(&state, epoch, active_app_context_cancellation.as_ref()) {
        cleanup_stale_finalize(&state, lease, temp_path);
        return;
    }
    // 与 start/cancel 串行化“最后一次 current 检查 + 注入 + 会话提交 + UI 终态”。
    // cancel 若先取得锁，本次不会注入；注入若已开始，则 cancel 会在其完成后收尾。
    let operation = state.dictation_runtime.operation.clone();
    let _guard = operation.lock().await;
    if !finalize_session_is_current(&state, epoch, active_app_context_cancellation.as_ref()) {
        cleanup_stale_finalize(&state, lease, temp_path);
        return;
    }
    let result = if processed.is_empty() {
        Ok(())
    } else {
        inject_text_inner(processed.clone(), Some(method)).await
    };
    if let Some(lease) = lease {
        let _ = state.audio_session.release(&lease);
    }
    remove_temp(temp_path);
    if let Err(e) = result {
        // fail 负责在同一把 operation 锁下提交失败终态，避免与 start/cancel 交错。
        drop(_guard);
        let _ = fail(app, epoch, e).await;
        return;
    }
    let committed = if let Ok(mut s) = state.dictation_runtime.session.lock() {
        if s.epoch == epoch
            && s.phase == DictationPhase::Injecting
            && !active_app_context_cancellation
                .as_ref()
                .is_some_and(|cancellation| cancellation.is_cancelled())
        {
            s.phase = DictationPhase::Idle;
            s.mode = None;
            s.public_id = None;
            s.file_job_id = None;
            s.committed.clear();
            s.segment.clear();
            s.active_app_context_cancellation = None;
            true
        } else {
            false
        }
    } else {
        false
    };
    if !committed {
        return;
    }
    hotkey::set_dictation_active(false);
    let _ = crate::desktop::set_indicator_state(app.clone(), "hidden".into());
    play_cue_async(app.clone(), "end", &prefs, CuePlaybackTarget::IndicatorWindow);
    publish_state_with_text(
        &app,
        None,
        processed,
        should_show_final_text_in_indicator(mode),
    );
}

async fn fail(app: AppHandle, epoch: u64, error: String) -> Result<(), String> {
    let state = app.state::<RuntimeState>();
    let operation = state.dictation_runtime.operation.clone();
    let _guard = operation.lock().await;
    let (lease, temp_path, asr, file_job, active_app_context, active_app_context_cancellation) = {
        let mut s = state
            .dictation_runtime
            .session
            .lock()
            .map_err(|_| "听写状态锁失败")?;
        // 迟到的错误不得把已经正常结束的同 epoch 会话重新标成失败，
        // 也无需重复提交已经失败会话的 UI 与资源清理。
        if s.epoch != epoch || matches!(s.phase, DictationPhase::Idle | DictationPhase::Failed) {
            return Ok(());
        }
        s.phase = DictationPhase::Failed;
        (
            s.lease.take(),
            s.temp_audio_path.take(),
            s.asr_session_id.take(),
            s.file_job_id.take(),
            s.active_app_context.take(),
            s.active_app_context_cancellation.take(),
        )
    };
    if let Some(cancellation) = active_app_context_cancellation {
        cancellation.cancel();
    }
    drop(active_app_context);
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
    let (text, show_in_indicator) = app
        .state::<RuntimeState>()
        .dictation_runtime
        .session
        .lock()
        .map(|s| {
            (
                format!("{}{}", s.committed, s.segment),
                should_show_final_text_in_indicator(s.mode),
            )
        })
        .unwrap_or_default();
    publish_state_with_text(app, error, text, show_in_indicator);
}
fn publish_state_with_text(
    app: &AppHandle,
    error: Option<String>,
    text: String,
    show_in_indicator: bool,
) {
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
        payload: json!({"phase": s.phase, "recording": matches!(s.phase, DictationPhase::Recording | DictationPhase::WaitingForVoice), "text": text, "error": error, "activeAppContext": state.active_app_context.latest_summary()}),
    };
    hotkey::set_dictation_active(!matches!(
        s.phase,
        DictationPhase::Idle | DictationPhase::Failed
    ));
    let _ = app.emit(DOMAIN_EVENT, event);
    if show_in_indicator && !text.is_empty() {
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

fn should_show_waveform(mode: Option<DictationMode>) -> bool {
    mode == Some(DictationMode::File)
}

fn should_show_final_text_in_indicator(mode: Option<DictationMode>) -> bool {
    mode != Some(DictationMode::File)
}

fn pcm16le_to_f32(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(2)
        .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]) as f32 / 32768.0)
        .collect()
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

/// 应用规则里显式开启该项的启用规则；用于判断某项配置是否需要校验。
fn profiles_enabling(
    prefs: &DictationPrefs,
    pick: impl Fn(&AppProfile) -> Option<bool>,
) -> impl Iterator<Item = &AppProfile> {
    let enabled = prefs.app_profiles_enabled;
    prefs
        .app_profiles
        .iter()
        .filter(move |profile| enabled && profile.enabled)
        .filter(move |profile| pick(profile) == Some(true))
}

fn validate_rules(prefs: &DictationPrefs) -> Result<(), String> {
    // 全局关闭但某条应用规则开启时，正则同样会被执行，必须一并校验。
    let needed = prefs.local_rules_enabled
        || profiles_enabling(prefs, |profile| profile.local_rules_enabled)
            .next()
            .is_some();
    if !needed {
        return Ok(());
    }
    for rule in prefs.local_rules.iter().filter(|r| r.enabled) {
        compile_rule(rule)?;
    }
    Ok(())
}

fn validate_template(prefs: &DictationPrefs, template_id: &str, scope: &str) -> Result<(), String> {
    let template = prefs
        .smart_templates
        .iter()
        .find(|template| template.id == template_id)
        .ok_or_else(|| format!("{scope}引用的智能处理模板不存在"))?;
    if template.name.trim().is_empty() {
        return Err("智能处理模板名称不能为空".to_string());
    }
    crate::application::smart_text::render_prompt(&template.prompt, "验证文本", "", "", "")?;
    Ok(())
}

fn validate_smart_processing(prefs: &DictationPrefs) -> Result<(), String> {
    if prefs.smart_templates.len() > MAX_SMART_TEMPLATES {
        return Err(format!("智能处理模板不能超过 {MAX_SMART_TEMPLATES} 个"));
    }
    if prefs.app_profiles.len() > MAX_APP_PROFILES {
        return Err(format!("软件规则不能超过 {MAX_APP_PROFILES} 条"));
    }
    if prefs.smart_processing_min_chars > MAX_SMART_PROCESSING_MIN_CHARS {
        return Err(format!(
            "智能处理最少字符数不能超过 {MAX_SMART_PROCESSING_MIN_CHARS}"
        ));
    }
    for profile in &prefs.app_profiles {
        if profile
            .smart_processing_min_chars
            .is_some_and(|value| value > MAX_SMART_PROCESSING_MIN_CHARS)
        {
            let name = if profile.name.trim().is_empty() {
                "软件规则"
            } else {
                profile.name.trim()
            };
            return Err(format!(
                "软件规则「{name}」的智能处理最少字符数不能超过 {MAX_SMART_PROCESSING_MIN_CHARS}"
            ));
        }
    }
    if prefs.smart_processing_enabled {
        prefs
            .smart_templates
            .iter()
            .find(|template| template.id == prefs.smart_template_id)
            .ok_or_else(|| "请选择有效的智能处理模板".to_string())?;
        validate_template(prefs, &prefs.smart_template_id, "全局配置")?;
    }
    // 规则引用的模板在保存时就要校验，否则要等到听写完成才失败。
    for profile in profiles_enabling(prefs, |profile| profile.smart_processing_enabled) {
        let Some(template_id) = profile
            .smart_template_id
            .as_deref()
            .filter(|id| !id.is_empty())
        else {
            continue;
        };
        let name = if profile.name.trim().is_empty() {
            "软件规则"
        } else {
            profile.name.trim()
        };
        validate_template(prefs, template_id, &format!("软件规则「{name}」"))?;
    }
    Ok(())
}
fn validate_ocr_preferences(prefs: &DictationPrefs) -> Result<(), String> {
    if prefs.active_app_context_ocr_model.len() > 128 {
        return Err("场景感知 OCR 模型 ID 过长".into());
    }
    if prefs.active_app_context_ocr_approved_providers.len() > 128 {
        return Err("场景感知 OCR 隐私授权记录过多".into());
    }
    let mut seen = std::collections::HashSet::new();
    for provider_id in &prefs.active_app_context_ocr_approved_providers {
        let valid = !provider_id.is_empty()
            && provider_id.len() <= 64
            && provider_id.bytes().all(|byte| {
                byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || matches!(byte, b'.' | b'-')
            });
        if !valid || !seen.insert(provider_id) {
            return Err(format!("场景感知 OCR 隐私授权供应商 ID 非法或重复：{provider_id}"));
        }
    }
    Ok(())
}
pub(crate) fn validate_dictation_settings_value(value: &Value) -> Result<(), String> {
    let prefs: DictationPrefs =
        serde_json::from_value(value.clone()).map_err(|e| format!("听写配置无效：{e}"))?;
    validate_rules(&prefs)?;
    validate_smart_processing(&prefs)?;
    validate_ocr_preferences(&prefs)
}

pub(crate) fn active_app_context_extraction_method_from_value(
    value: &Value,
) -> crate::active_app_context::ActiveAppContextExtractionMethod {
    serde_json::from_value::<DictationPrefs>(value.clone())
        .map(|prefs| prefs.active_app_context_extraction_method)
        .unwrap_or_default()
}

pub(crate) fn active_app_context_ocr_engine_from_value(
    value: &Value,
) -> crate::active_app_context::OcrEngineKind {
    serde_json::from_value::<DictationPrefs>(value.clone())
        .map(|prefs| prefs.active_app_context_ocr_engine)
        .unwrap_or_default()
}

pub(crate) fn active_app_context_ocr_model_from_value(value: &Value) -> String {
    serde_json::from_value::<DictationPrefs>(value.clone())
        .map(|prefs| prefs.active_app_context_ocr_model)
        .unwrap_or_default()
}

pub(crate) fn active_app_context_ocr_approved_providers_from_value(
    value: &Value,
) -> Vec<String> {
    serde_json::from_value::<DictationPrefs>(value.clone())
        .map(|prefs| prefs.active_app_context_ocr_approved_providers)
        .unwrap_or_default()
}

pub(crate) fn resolve_active_app_context_ocr_provider(
    state: &RuntimeState,
    selected_model: &str,
    legacy_engine: crate::active_app_context::OcrEngineKind,
    approved_providers: &[String],
) -> crate::providers::capabilities::OcrProvider {
    use crate::providers::capabilities::OcrProvider;

    let selected = selected_model.trim();
    if selected.is_empty() {
        return match legacy_engine {
            crate::active_app_context::OcrEngineKind::System => OcrProvider::System,
            crate::active_app_context::OcrEngineKind::PpOcr => {
                let spec = state
                    .plugin_registry
                    .lock()
                    .ok()
                    .and_then(|registry| registry.local_model_for_engine("ppocr-mnn"));
                let spec = spec.filter(|spec| {
                    state.providers.lock().is_ok_and(|settings| {
                        settings.profiles.iter().any(|profile| {
                            profile.id == spec.provider_id && profile.enabled
                        })
                    })
                });
                spec.map(|spec| OcrProvider::PpOcr { spec }).unwrap_or_else(|| OcrProvider::Unavailable {
                    selection: "ppocr".into(),
                    reason: "旧版 PP-OCR 设置已迁移，但本地模型包尚未安装；请在插件管理安装 PP-OCRv6 Tiny 后重新选择。".into(),
                })
            }
        };
    }
    if selected == crate::providers::SYSTEM_OCR_PROVIDER_ID || selected == "system" {
        return OcrProvider::System;
    }

    let settings = match crate::commands::common::read_provider_settings(state) {
        Ok(settings) => settings,
        Err(error) => {
            return OcrProvider::Unavailable {
                selection: selected.into(),
                reason: error,
            }
        }
    };
    let registry = match state.plugin_registry.lock() {
        Ok(registry) => registry,
        Err(_) => {
            return OcrProvider::Unavailable {
                selection: selected.into(),
                reason: "插件注册表锁失败".into(),
            }
        }
    };
    let local = registry.local_model_for_model(selected);
    let provider_id = local
        .as_ref()
        .map(|spec| spec.provider_id.clone())
        .or_else(|| registry.provider_id_for_model(selected))
        .or_else(|| {
            settings
                .profiles
                .iter()
                .find(|profile| profile.id == selected)
                .map(|profile| profile.id.clone())
        });
    let Some(provider_id) = provider_id else {
        return OcrProvider::Unavailable {
            selection: selected.into(),
            reason: format!("OCR 模型 {selected} 不可用；请安装对应模型包或重新选择。"),
        };
    };
    let Some(profile) = settings.profiles.iter().find(|profile| {
        profile.id == provider_id
            && profile.enabled
            && profile.capabilities.iter().any(|capability| capability == "ocr")
    }) else {
        return OcrProvider::Unavailable {
            selection: selected.into(),
            reason: format!("OCR 模型 {selected} 的供应商未启用或已卸载。"),
        };
    };
    if let Some(spec) = local {
        return if spec.engine == "ppocr-mnn" {
            OcrProvider::PpOcr { spec }
        } else {
            OcrProvider::Unavailable {
                selection: selected.into(),
                reason: format!("模型引擎 {} 不支持场景感知 OCR。", spec.engine),
            }
        };
    }
    let runtime = match registry.runtime_for_provider(&provider_id) {
        Ok(Some(runtime)) => runtime,
        Ok(None) => {
            return OcrProvider::Unavailable {
                selection: selected.into(),
                reason: format!("OCR 供应商 {provider_id} 没有可调用的识别运行时。"),
            }
        }
        Err(error) => {
            return OcrProvider::Unavailable {
                selection: selected.into(),
                reason: error,
            }
        }
    };
    if !approved_providers.iter().any(|id| id == &provider_id) {
        return OcrProvider::Unavailable {
            selection: selected.into(),
            reason: format!("尚未确认向第三方 OCR 供应商 {provider_id} 发送场景感知截图。"),
        };
    }
    OcrProvider::Plugin {
        spec: runtime,
        profile: profile.clone(),
    }
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
fn apply_rules(text: &str, prefs: &DictationPrefs, enabled: bool) -> Result<String, String> {
    if !enabled {
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

fn play_cue_async(
    app: AppHandle,
    which: &'static str,
    prefs: &DictationPrefs,
    target: CuePlaybackTarget,
) {
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
    let event = match target {
        CuePlaybackTarget::MainWindow => "dictation-play-cue",
        CuePlaybackTarget::IndicatorWindow => "dictation-indicator-play-cue",
    };
    tauri::async_runtime::spawn(async move {
        // 第一次创建悬浮窗时，给它的 WebView 留出注册事件监听器的时间。
        if matches!(target, CuePlaybackTarget::IndicatorWindow) {
            sleep(Duration::from_millis(100)).await;
        }
        if let CuePlaybackTarget::IndicatorWindow = target {
            if let Some(window) = app.get_webview_window("dictation-indicator") {
                let _ = window.emit(event, json!({ "which": which, "kind": kind }));
                return;
            }
        }
        let _ = app.emit(event, json!({ "which": which, "kind": kind }));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ocr_follow_min_chars_is_enabled_for_new_defaults_but_not_legacy_json() {
        assert!(DictationPrefs::default().active_app_context_ocr_follow_smart_processing_min_chars);
        let legacy: DictationPrefs = serde_json::from_value(json!({})).unwrap();
        assert!(!legacy.active_app_context_ocr_follow_smart_processing_min_chars);
    }

    #[test]
    fn ocr_is_deferred_only_for_an_enabled_positive_minimum() {
        let effective = EffectivePostProcessing {
            local_rules_enabled: false,
            smart_processing_enabled: true,
            smart_processing_min_chars: 20,
            smart_template_id: String::new(),
            matched_profile: None,
        };
        let mut prefs = DictationPrefs {
            active_app_context_extraction_method:
                crate::active_app_context::ActiveAppContextExtractionMethod::Ocr,
            active_app_context_ocr_follow_smart_processing_min_chars: true,
            ..Default::default()
        };
        assert!(should_defer_active_app_context_ocr(&prefs, &effective));

        let mut immediate = effective.clone();
        immediate.smart_processing_min_chars = 0;
        assert!(!should_defer_active_app_context_ocr(&prefs, &immediate));
        prefs.active_app_context_extraction_method =
            crate::active_app_context::ActiveAppContextExtractionMethod::NativeText;
        assert!(!should_defer_active_app_context_ocr(&prefs, &effective));
        prefs.active_app_context_extraction_method =
            crate::active_app_context::ActiveAppContextExtractionMethod::Ocr;
        prefs.active_app_context_ocr_follow_smart_processing_min_chars = false;
        assert!(!should_defer_active_app_context_ocr(&prefs, &effective));
    }

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
        assert_eq!(apply_rules("中AA", &p, true).unwrap(), "中A");
    }
    fn identity(process_name: &str) -> crate::active_app_context::AppIdentity {
        crate::active_app_context::AppIdentity {
            process_name: process_name.into(),
            app_name: process_name.trim_end_matches(".exe").into(),
            window_title: None,
        }
    }

    fn prefs_with_profiles(profiles: Vec<AppProfile>) -> DictationPrefs {
        DictationPrefs {
            local_rules_enabled: true,
            smart_processing_enabled: false,
            smart_processing_min_chars: 120,
            smart_template_id: "global".into(),
            app_profiles_enabled: true,
            app_profiles: profiles,
            ..Default::default()
        }
    }

    #[test]
    fn unmatched_app_falls_back_to_global_post_processing() {
        let prefs = prefs_with_profiles(vec![AppProfile {
            matchers: vec!["Code.exe".into()],
            smart_processing_enabled: Some(true),
            ..Default::default()
        }]);
        let effective = prefs.resolve_post_processing(Some(&identity("notepad.exe")));
        assert!(effective.local_rules_enabled);
        assert!(!effective.smart_processing_enabled);
        assert_eq!(effective.smart_template_id, "global");
        assert!(effective.matched_profile.is_none());
    }

    #[test]
    fn profile_overrides_only_the_fields_it_sets() {
        let prefs = prefs_with_profiles(vec![AppProfile {
            name: "编程".into(),
            matchers: vec!["code.exe".into()],
            smart_processing_enabled: Some(true),
            smart_template_id: Some("coding".into()),
            ..Default::default()
        }]);
        // 大小写不敏感，且未设置的本地处理仍继承全局的 true。
        let effective = prefs.resolve_post_processing(Some(&identity("Code.exe")));
        assert!(effective.local_rules_enabled);
        assert!(effective.smart_processing_enabled);
        assert_eq!(effective.smart_processing_min_chars, 120);
        assert_eq!(effective.smart_template_id, "coding");
        assert_eq!(effective.matched_profile.as_deref(), Some("编程"));
    }

    #[test]
    fn profile_can_override_smart_processing_min_chars() {
        let mut prefs = prefs_with_profiles(vec![AppProfile {
            matchers: vec!["code.exe".into()],
            smart_processing_enabled: Some(true),
            smart_processing_min_chars: Some(0),
            ..Default::default()
        }]);
        prefs.active_app_context_extraction_method =
            crate::active_app_context::ActiveAppContextExtractionMethod::Ocr;
        let effective = prefs.resolve_post_processing(Some(&identity("code.exe")));
        assert_eq!(effective.smart_processing_min_chars, 0);
        assert!(should_run_smart_processing(&effective, "短句"));
        assert!(!should_defer_active_app_context_ocr(&prefs, &effective));
    }

    #[test]
    fn smart_processing_min_chars_uses_unicode_character_boundary() {
        let effective = EffectivePostProcessing {
            local_rules_enabled: false,
            smart_processing_enabled: true,
            smart_processing_min_chars: 4,
            smart_template_id: String::new(),
            matched_profile: None,
        };
        assert!(!should_run_smart_processing(&effective, " 你好世 "));
        assert!(should_run_smart_processing(&effective, " 你好世界 "));
    }

    #[test]
    fn smart_processing_min_chars_is_validated_for_profiles() {
        let prefs = prefs_with_profiles(vec![AppProfile {
            name: "编程".into(),
            smart_processing_min_chars: Some(MAX_SMART_PROCESSING_MIN_CHARS + 1),
            ..Default::default()
        }]);
        let error = validate_smart_processing(&prefs).unwrap_err();
        assert!(error.contains("编程"));
    }

    #[test]
    fn first_enabled_matching_profile_wins() {
        let prefs = prefs_with_profiles(vec![
            AppProfile {
                name: "停用的".into(),
                matchers: vec!["code.exe".into()],
                enabled: false,
                local_rules_enabled: Some(false),
                ..Default::default()
            },
            AppProfile {
                name: "第一条".into(),
                matchers: vec!["code.exe".into()],
                smart_template_id: Some("first".into()),
                ..Default::default()
            },
            AppProfile {
                name: "第二条".into(),
                matchers: vec!["code.exe".into()],
                smart_template_id: Some("second".into()),
                ..Default::default()
            },
        ]);
        let effective = prefs.resolve_post_processing(Some(&identity("code.exe")));
        assert_eq!(effective.matched_profile.as_deref(), Some("第一条"));
        assert_eq!(effective.smart_template_id, "first");
    }

    #[test]
    fn profiles_are_ignored_when_disabled_or_identity_is_unknown() {
        let mut prefs = prefs_with_profiles(vec![AppProfile {
            matchers: vec!["code.exe".into()],
            local_rules_enabled: Some(false),
            ..Default::default()
        }]);
        // 平台拿不到前台软件时回落全局。
        assert!(prefs.resolve_post_processing(None).local_rules_enabled);
        prefs.app_profiles_enabled = false;
        assert!(
            prefs
                .resolve_post_processing(Some(&identity("code.exe")))
                .local_rules_enabled
        );
    }

    #[test]
    fn profile_referenced_template_must_exist() {
        let mut prefs = prefs_with_profiles(vec![AppProfile {
            name: "编程".into(),
            matchers: vec!["code.exe".into()],
            smart_processing_enabled: Some(true),
            smart_template_id: Some("missing".into()),
            ..Default::default()
        }]);
        prefs.smart_templates = vec![SmartTemplate {
            id: "global".into(),
            name: "全局".into(),
            prompt: format!("处理：{}", crate::application::smart_text::TEXT_PLACEHOLDER),
        }];
        let error = validate_smart_processing(&prefs).unwrap_err();
        assert!(error.contains("编程"), "错误信息应指出是哪条规则：{error}");
    }

    #[test]
    fn local_rules_are_validated_when_only_a_profile_enables_them() {
        let mut prefs = prefs_with_profiles(vec![AppProfile {
            matchers: vec!["code.exe".into()],
            local_rules_enabled: Some(true),
            ..Default::default()
        }]);
        prefs.local_rules_enabled = false;
        prefs.local_rules = vec![LocalRule {
            id: "bad".into(),
            enabled: true,
            mode: String::new(),
            find: String::new(),
            pattern: "([".into(),
            flags: "g".into(),
            replacement: String::new(),
        }];
        assert!(validate_rules(&prefs).is_err());
    }

    #[test]
    fn stale_epoch_does_not_match() {
        let mut s = Session::default();
        s.epoch = 2;
        assert!(!s.is_current(1));
    }
    #[test]
    fn waveform_is_reserved_for_file_dictation() {
        assert!(should_show_waveform(Some(DictationMode::File)));
        assert!(!should_show_waveform(Some(DictationMode::Realtime)));
        assert!(!should_show_waveform(None));
    }
    #[test]
    fn file_dictation_never_projects_final_text_to_indicator() {
        assert!(!should_show_final_text_in_indicator(Some(
            DictationMode::File
        )));
        assert!(should_show_final_text_in_indicator(Some(
            DictationMode::Realtime
        )));
    }
    #[test]
    fn smart_template_limit_applies_while_processing_is_disabled() {
        let mut prefs = DictationPrefs::default();
        prefs.smart_templates = (0..=MAX_SMART_TEMPLATES)
            .map(|index| SmartTemplate {
                id: format!("template-{index}"),
                name: format!("模板 {index}"),
                prompt: "{{text}}".into(),
            })
            .collect();
        assert!(validate_smart_processing(&prefs).is_err());
    }
    #[test]
    fn waveform_preview_decodes_processed_pcm() {
        assert_eq!(
            pcm16le_to_f32(&[0, 0, 255, 127, 0, 128]),
            vec![0.0, i16::MAX as f32 / 32768.0, -1.0]
        );
    }

    #[test]
    fn legacy_ppocr_without_model_pack_is_unavailable_but_system_ocr_remains_available() {
        let state = RuntimeState::default();
        assert!(matches!(
            resolve_active_app_context_ocr_provider(
                &state,
                "",
                crate::active_app_context::OcrEngineKind::PpOcr,
                &[],
            ),
            crate::providers::capabilities::OcrProvider::Unavailable { .. }
        ));
        assert!(matches!(
            resolve_active_app_context_ocr_provider(
                &state,
                crate::providers::SYSTEM_OCR_PROVIDER_ID,
                crate::active_app_context::OcrEngineKind::PpOcr,
                &[],
            ),
            crate::providers::capabilities::OcrProvider::System
        ));
    }

    #[test]
    fn remote_ocr_requires_provider_specific_privacy_approval() {
        let root = std::env::temp_dir().join(format!("sayit-ocr-privacy-{}", Uuid::new_v4()));
        let plugin = root.join("remote-ocr");
        std::fs::create_dir_all(plugin.join("connector")).unwrap();
        std::fs::write(
            plugin.join("connector/index.js"),
            b"export function recognizeImage() { return { blocks: [] }; }",
        )
        .unwrap();
        std::fs::write(
            plugin.join("manifest.json"),
            serde_json::to_vec(&serde_json::json!({
                "apiVersion": 4,
                "id": "remote-ocr",
                "name": "Remote OCR",
                "version": "1.0.0",
                "provider": {
                    "id": "remote-ocr",
                    "displayName": "Remote OCR",
                    "authKind": "none",
                    "capabilities": ["ocr"],
                    "config": {}
                },
                "models": [],
                "runtime": {
                    "kind": "javascript",
                    "entrypoint": "connector/index.js",
                    "hostApiVersion": 1
                }
            }))
            .unwrap(),
        )
        .unwrap();
        let registry = crate::providers::plugin::load_registry_from(&root).unwrap();
        let mut settings = crate::providers::ProviderSettings::default();
        registry.merge_provider_profiles(&mut settings);
        let state = RuntimeState::default();
        *state.plugin_registry.lock().unwrap() = registry;
        *state.providers.lock().unwrap() = settings;

        assert!(matches!(
            resolve_active_app_context_ocr_provider(
                &state,
                "remote-ocr",
                crate::active_app_context::OcrEngineKind::System,
                &[],
            ),
            crate::providers::capabilities::OcrProvider::Unavailable { .. }
        ));
        assert!(matches!(
            resolve_active_app_context_ocr_provider(
                &state,
                "remote-ocr",
                crate::active_app_context::OcrEngineKind::System,
                &["remote-ocr".into()],
            ),
            crate::providers::capabilities::OcrProvider::Plugin { .. }
        ));
        std::fs::remove_dir_all(root).unwrap();
    }
}
