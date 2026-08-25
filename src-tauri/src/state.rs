use crate::obs_overlay::{ObsOverlayRuntime, ObsOverlaySettings};
use crate::prelude::*;
use std::sync::atomic::AtomicU64;

#[derive(Default)]
pub(crate) struct RuntimeState {
    pub(crate) snapshot_revision: AtomicU64,
    pub(crate) app_settings: Mutex<crate::application::settings::AppSettings>,
    pub(crate) providers: Mutex<ProviderSettings>,
    pub(crate) plugin_registry: Mutex<crate::providers::plugin::PluginRegistry>,
    pub(crate) pending_plugin_imports: Mutex<VecDeque<String>>,
    pub(crate) asr_streams: Arc<Mutex<HashMap<String, AsrStreamHandle>>>,
    pub(crate) transcriptions: Arc<Mutex<HashMap<String, Arc<std::sync::atomic::AtomicBool>>>>,
    pub(crate) dictation: Mutex<DictationSettings>,
    pub(crate) subtitle_shortcut: Mutex<SubtitleShortcutSettings>,
    pub(crate) assistant_shortcuts: Mutex<crate::application::assistant::AssistantShortcutSettings>,
    pub(crate) shortcut_config_operation: Mutex<()>,
    pub(crate) subtitle_translation_model: Mutex<String>,
    pub(crate) startup: Mutex<StartupSettings>,
    pub(crate) backend_mic: Arc<Mutex<BackendMicState>>,
    pub(crate) backend_events: crate::application::events::BackendEventHub,
    pub(crate) audio_session: crate::application::audio_session::AudioSessionCoordinator,
    pub(crate) legacy_audio_lease: Mutex<Option<crate::application::audio_session::AudioLease>>,
    pub(crate) dictation_runtime: crate::application::dictation::DictationRuntime,
    pub(crate) active_app_context: crate::active_app_context::ContextCaptureService,
    pub(crate) subtitle_runtime: crate::application::subtitles::SubtitleRuntime,
    pub(crate) transcription_runtime: crate::application::transcription::TranscriptionRuntime,
    pub(crate) compare_runtime: crate::application::compare::CompareRuntime,
    pub(crate) audio_lab_runtime: crate::application::audio_lab::AudioLabRuntime,
    pub(crate) assistant_runtime: crate::application::assistant::AssistantRuntime,
    pub(crate) correction_samples: Mutex<Vec<crate::application::history::CorrectionSample>>,
    pub(crate) audio_lab_lease: Mutex<Option<crate::application::audio_session::AudioLease>>,
    pub(crate) main_window_lifecycle:
        Mutex<crate::application::window_lifecycle::MainWindowLifecycle>,
    /// 实时字幕"系统音频"来源用的 loopback 采集状态，和麦克风共用同一套结构体但各自独立。
    pub(crate) backend_system_audio: Arc<Mutex<BackendMicState>>,
    pub(crate) main_window_placement: Mutex<Option<MainWindowPlacement>>,
    pub(crate) floating_orb: Mutex<FloatingOrbSettings>,
    pub(crate) floating_orb_runtime: FloatingOrbRuntime,
    pub(crate) mouse_gesture: Mutex<MouseGestureSettings>,
    pub(crate) mouse_gesture_runtime: MouseGestureRuntime,
    pub(crate) obs_overlay_settings: Mutex<ObsOverlaySettings>,
    pub(crate) obs_overlay_runtime: ObsOverlayRuntime,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FloatingOrbPosition {
    pub(crate) x: i32,
    pub(crate) y: i32,
}

/// 悬浮球大小，单位为屏幕参考边长的十分之一百分比（如 45 表示 4.5%）。
pub(crate) const DEFAULT_FLOATING_ORB_SIZE_PERCENT: u16 = 45;
pub(crate) const DEFAULT_FLOATING_ORB_OPACITY: u8 = 100;
pub(crate) const DEFAULT_FLOATING_ORB_GLASS_TINT: u8 = 8;
pub(crate) const DEFAULT_FLOATING_ORB_GLASS_BORDER: u8 = 0;
pub(crate) const DEFAULT_MOUSE_GESTURE_SENSITIVITY: u8 = 50;
pub(crate) const DEFAULT_MOUSE_RAPID_CLICK_COUNT: u8 = 4;
pub(crate) const MIN_MOUSE_RAPID_CLICK_COUNT: u8 = 3;
pub(crate) const MAX_MOUSE_RAPID_CLICK_COUNT: u8 = 10;

fn default_floating_orb_size_percent() -> u16 {
    DEFAULT_FLOATING_ORB_SIZE_PERCENT
}

fn default_floating_orb_opacity() -> u8 {
    DEFAULT_FLOATING_ORB_OPACITY
}

fn default_floating_orb_glass_tint() -> u8 {
    DEFAULT_FLOATING_ORB_GLASS_TINT
}

fn default_floating_orb_glass_border() -> u8 {
    DEFAULT_FLOATING_ORB_GLASS_BORDER
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum FloatingOrbGlassMaterial {
    UnderWindow,
    Content,
    #[default]
    Sidebar,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FloatingOrbSettings {
    #[serde(default)]
    pub(crate) enabled: bool,
    #[serde(default)]
    pub(crate) position: Option<FloatingOrbPosition>,
    /// 屏幕参考边长（较短逻辑边）的十分之一百分比，如 45 表示 4.5%；
    /// 换算为实际像素时综合考虑显示器分辨率与缩放比例，详见
    /// desktop::floating_orb::orb_window_extent。
    #[serde(default = "default_floating_orb_size_percent")]
    pub(crate) size_percent: u16,
    #[serde(default = "default_floating_orb_opacity")]
    pub(crate) opacity: u8,
    #[serde(default)]
    pub(crate) glass_enabled: bool,
    #[serde(default)]
    pub(crate) glass_material: FloatingOrbGlassMaterial,
    #[serde(default = "default_floating_orb_glass_tint")]
    pub(crate) glass_tint: u8,
    #[serde(default = "default_floating_orb_glass_border")]
    pub(crate) glass_border: u8,
    #[serde(default)]
    pub(crate) auto_enter: bool,
}

impl Default for FloatingOrbSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            position: None,
            size_percent: DEFAULT_FLOATING_ORB_SIZE_PERCENT,
            opacity: DEFAULT_FLOATING_ORB_OPACITY,
            glass_enabled: false,
            glass_material: FloatingOrbGlassMaterial::default(),
            glass_tint: DEFAULT_FLOATING_ORB_GLASS_TINT,
            glass_border: DEFAULT_FLOATING_ORB_GLASS_BORDER,
            auto_enter: false,
        }
    }
}

pub(crate) struct FloatingOrbRuntime {
    pub(crate) placement_generation: AtomicU64,
    pub(crate) appearance_generation: AtomicU64,
    pub(crate) transition_generation: AtomicU64,
    pub(crate) suppress_main_reopen_until_ms: AtomicU64,
    pub(crate) transient: std::sync::atomic::AtomicBool,
    pub(crate) armed: std::sync::atomic::AtomicBool,
    pub(crate) armed_generation: AtomicU64,
    pub(crate) armed_target: Mutex<Option<crate::active_app_context::ActivationTarget>>,
    pub(crate) post_injection_action: Mutex<Option<FloatingOrbPostInjectionAction>>,
}

impl Default for FloatingOrbRuntime {
    fn default() -> Self {
        Self {
            placement_generation: AtomicU64::new(0),
            appearance_generation: AtomicU64::new(0),
            transition_generation: AtomicU64::new(0),
            suppress_main_reopen_until_ms: AtomicU64::new(0),
            transient: std::sync::atomic::AtomicBool::new(false),
            armed: std::sync::atomic::AtomicBool::new(false),
            armed_generation: AtomicU64::new(0),
            armed_target: Mutex::new(None),
            post_injection_action: Mutex::new(None),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct FloatingOrbPostInjectionAction {
    pub(crate) target: crate::active_app_context::ActivationTarget,
    pub(crate) expires_at: Instant,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum MouseGestureMode {
    #[default]
    Confirm,
    Direct,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MouseGestureSettings {
    #[serde(default)]
    pub(crate) enabled: bool,
    #[serde(default)]
    pub(crate) mode: MouseGestureMode,
    #[serde(default = "default_mouse_gesture_sensitivity")]
    pub(crate) sensitivity: u8,
    #[serde(default = "default_mouse_rapid_click_enabled")]
    pub(crate) rapid_click_enabled: bool,
    #[serde(default = "default_mouse_rapid_click_count")]
    pub(crate) rapid_click_count: u8,
}

fn default_mouse_gesture_sensitivity() -> u8 {
    DEFAULT_MOUSE_GESTURE_SENSITIVITY
}

fn default_mouse_rapid_click_enabled() -> bool {
    true
}

fn default_mouse_rapid_click_count() -> u8 {
    DEFAULT_MOUSE_RAPID_CLICK_COUNT
}

impl Default for MouseGestureSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: MouseGestureMode::Confirm,
            sensitivity: DEFAULT_MOUSE_GESTURE_SENSITIVITY,
            rapid_click_enabled: true,
            rapid_click_count: DEFAULT_MOUSE_RAPID_CLICK_COUNT,
        }
    }
}

impl MouseGestureSettings {
    pub(crate) fn normalized(mut self) -> Self {
        self.sensitivity = self.sensitivity.min(100);
        self.rapid_click_count = self
            .rapid_click_count
            .clamp(MIN_MOUSE_RAPID_CLICK_COUNT, MAX_MOUSE_RAPID_CLICK_COUNT);
        self
    }
}

#[derive(Default)]
pub(crate) struct MouseGestureRuntime {
    pub(crate) listening: std::sync::atomic::AtomicBool,
    pub(crate) error: Mutex<Option<String>>,
}

#[derive(Default)]
pub(crate) struct BackendMicState {
    pub(crate) worker: Option<std::sync::mpsc::Sender<BackendMicCommand>>,
    pub(crate) sample_rate: u32,
    pub(crate) channels: usize,
    pub(crate) session_id: Option<String>,
    pub(crate) tx: Option<tokio::sync::mpsc::UnboundedSender<AsrStreamInput>>,
    pub(crate) raw_txs: Vec<tokio::sync::mpsc::UnboundedSender<AsrStreamInput>>,
    pub(crate) pending: VecDeque<Vec<f32>>,
    pub(crate) buffer: Vec<f32>,
    pub(crate) chunk_count: u64,
    pub(crate) last_rms: f32,
    /// 采集流意外终止时保留原始错误，供上层在原始音频通道关闭后展示。
    pub(crate) last_error: Option<String>,
    /// 当前 worker 实际打开的设备名；`None` 表示用的是系统默认设备。
    pub(crate) current_device: Option<String>,
}

pub(crate) enum BackendMicCommand {
    Attach {
        session_id: String,
        tx: tokio::sync::mpsc::UnboundedSender<AsrStreamInput>,
        reply: std::sync::mpsc::Sender<Result<BackendMicAttachResponse, String>>,
    },
    AttachRaw {
        tx: tokio::sync::mpsc::UnboundedSender<AsrStreamInput>,
        reply: std::sync::mpsc::Sender<Result<BackendMicAttachResponse, String>>,
    },
    Pause {
        reply: std::sync::mpsc::Sender<Result<usize, String>>,
    },
    CaptureError {
        message: String,
    },
    /// `reply` 在设备真正释放、guard 状态清理完成后才会收到信号，
    /// 用于切换设备时确保旧 worker 完全退出后再起新的，避免状态被旧线程的收尾逻辑覆盖。
    Stop {
        reply: Option<std::sync::mpsc::Sender<()>>,
    },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BackendMicStartResponse {
    pub(crate) sample_rate: u32,
    pub(crate) channels: usize,
    pub(crate) reused: bool,
    /// 实际打开的设备名；`None` 表示默认设备。
    pub(crate) device_name: Option<String>,
    /// 请求的设备没找到（比如已拔出），已回退到默认设备。
    pub(crate) fallback: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BackendMicAttachResponse {
    pub(crate) flushed_chunks: usize,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct MainWindowPlacement {
    pub(crate) position: tauri::PhysicalPosition<i32>,
    pub(crate) size: tauri::LogicalSize<f64>,
    pub(crate) maximized: bool,
}

pub(crate) fn default_key_code() -> String {
    "CapsLock".to_string()
}

pub(crate) fn default_inject_method() -> String {
    "paste".to_string()
}

pub(crate) const MAX_DICTATION_SHORTCUT_PROFILES: usize = 8;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ShortcutProcessingMode {
    #[default]
    FollowScene,
    Raw,
    LocalOnly,
    SmartOnly,
    SmartAndLocal,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ShortcutTriggerMode {
    #[default]
    Toggle,
    PressHold,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DictationShortcutProfile {
    #[serde(default)]
    pub(crate) id: String,
    #[serde(default)]
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) enabled: bool,
    #[serde(default)]
    pub(crate) key_code: String,
    #[serde(default)]
    pub(crate) ctrl: bool,
    #[serde(default)]
    pub(crate) shift: bool,
    #[serde(default)]
    pub(crate) alt: bool,
    #[serde(default)]
    pub(crate) meta: bool,
    #[serde(default)]
    pub(crate) processing_mode: ShortcutProcessingMode,
    #[serde(default)]
    pub(crate) trigger_mode: ShortcutTriggerMode,
    #[serde(default)]
    pub(crate) smart_template_id: Option<String>,
    #[serde(default)]
    pub(crate) smart_processing_min_chars: Option<u32>,
    #[serde(default)]
    pub(crate) inject_method: Option<String>,
}

impl DictationShortcutProfile {
    pub(crate) fn mods(&self) -> u8 {
        hotkey_mods(self.ctrl, self.shift, self.alt, self.meta)
    }

    pub(crate) fn press_hold_mode(&self) -> bool {
        self.trigger_mode == ShortcutTriggerMode::PressHold
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DictationSettings {
    #[serde(default = "default_key_code")]
    pub(crate) key_code: String,
    #[serde(default)]
    pub(crate) ctrl: bool,
    #[serde(default)]
    pub(crate) shift: bool,
    #[serde(default)]
    pub(crate) alt: bool,
    #[serde(default)]
    pub(crate) meta: bool,
    #[serde(default = "default_inject_method")]
    pub(crate) inject_method: String,
    #[serde(default)]
    pub(crate) press_hold_mode: bool,
    #[serde(default)]
    pub(crate) shortcut_profiles: Vec<DictationShortcutProfile>,
}

impl Default for DictationSettings {
    fn default() -> Self {
        Self {
            key_code: default_key_code(),
            ctrl: false,
            shift: false,
            alt: false,
            meta: false,
            inject_method: default_inject_method(),
            press_hold_mode: false,
            shortcut_profiles: Vec::new(),
        }
    }
}

pub(crate) fn dictation_mods(settings: &DictationSettings) -> u8 {
    hotkey_mods(settings.ctrl, settings.shift, settings.alt, settings.meta)
}

fn hotkey_mods(ctrl: bool, shift: bool, alt: bool, meta: bool) -> u8 {
    let mut mods = 0u8;
    if ctrl {
        mods |= hotkey::MOD_CTRL;
    }
    if shift {
        mods |= hotkey::MOD_SHIFT;
    }
    if alt {
        mods |= hotkey::MOD_ALT;
    }
    if meta {
        mods |= hotkey::MOD_WIN;
    }
    mods
}

/// 应用语音输入热键；key_code 为空表示未设置，直接清除即可。
pub(crate) fn apply_dictation_hotkey(settings: &DictationSettings) -> Result<(), String> {
    let mut bindings = Vec::with_capacity(1 + settings.shortcut_profiles.len());
    if !settings.key_code.trim().is_empty() {
        let vk = hotkey::code_to_vk(&settings.key_code)
            .ok_or_else(|| format!("不支持的按键：{}", settings.key_code))?;
        bindings.push(hotkey::HotkeyBinding {
            vk,
            mods: dictation_mods(settings),
            profile_id: None,
            press_hold_mode: settings.press_hold_mode,
        });
    }
    for profile in settings
        .shortcut_profiles
        .iter()
        .filter(|profile| profile.enabled)
    {
        let vk = hotkey::code_to_vk(&profile.key_code)
            .ok_or_else(|| format!("快捷键方案「{}」使用了不支持的按键", profile.name))?;
        bindings.push(hotkey::HotkeyBinding {
            vk,
            mods: profile.mods(),
            profile_id: Some(profile.id.clone()),
            press_hold_mode: profile.press_hold_mode(),
        });
    }
    hotkey::set_hotkeys(&bindings)
}

#[cfg(test)]
mod dictation_settings_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn default_dictation_shortcut_is_caps_lock_without_modifiers() {
        let settings = DictationSettings::default();
        assert_eq!(settings.key_code, "CapsLock");
        assert!(!settings.ctrl && !settings.shift && !settings.alt && !settings.meta);
    }

    #[test]
    fn legacy_dictation_settings_migrate_to_no_shortcut_profiles() {
        let settings: DictationSettings = serde_json::from_value(json!({
            "key_code": "CapsLock",
            "inject_method": "paste"
        }))
        .unwrap();
        assert_eq!(settings.key_code, "CapsLock");
        assert!(settings.shortcut_profiles.is_empty());
    }

    #[test]
    fn shortcut_profile_uses_camel_case_nested_contract() {
        let settings: DictationSettings = serde_json::from_value(json!({
            "shortcut_profiles": [{
                "id": "smart",
                "name": "智能",
                "enabled": true,
                "keyCode": "F9",
                "processingMode": "smartOnly",
                "triggerMode": "pressHold",
                "smartProcessingMinChars": 0,
                "injectMethod": "type"
            }]
        }))
        .unwrap();
        let profile = &settings.shortcut_profiles[0];
        assert_eq!(profile.processing_mode, ShortcutProcessingMode::SmartOnly);
        assert_eq!(profile.trigger_mode, ShortcutTriggerMode::PressHold);
        assert_eq!(profile.smart_processing_min_chars, Some(0));
        assert_eq!(profile.inject_method.as_deref(), Some("type"));
    }

    #[test]
    fn legacy_shortcut_profile_defaults_to_toggle_trigger() {
        let settings: DictationSettings = serde_json::from_value(json!({
            "shortcut_profiles": [{
                "id": "legacy",
                "name": "旧方案",
                "enabled": false,
                "keyCode": "F10"
            }]
        }))
        .unwrap();
        assert_eq!(
            settings.shortcut_profiles[0].trigger_mode,
            ShortcutTriggerMode::Toggle
        );
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub(crate) struct SubtitleShortcutSettings {
    #[serde(default)]
    pub(crate) key_code: String,
    #[serde(default)]
    pub(crate) ctrl: bool,
    #[serde(default)]
    pub(crate) shift: bool,
    #[serde(default)]
    pub(crate) alt: bool,
    #[serde(default)]
    pub(crate) meta: bool,
}

pub(crate) fn subtitle_shortcut_mods(settings: &SubtitleShortcutSettings) -> u8 {
    let mut mods = 0u8;
    if settings.ctrl {
        mods |= hotkey::MOD_CTRL;
    }
    if settings.shift {
        mods |= hotkey::MOD_SHIFT;
    }
    if settings.alt {
        mods |= hotkey::MOD_ALT;
    }
    if settings.meta {
        mods |= hotkey::MOD_WIN;
    }
    mods
}

/// 应用实时字幕热键；key_code 为空表示未设置，直接清除即可。
pub(crate) fn apply_subtitle_hotkey(settings: &SubtitleShortcutSettings) -> Result<(), String> {
    if settings.key_code.trim().is_empty() {
        hotkey::clear_subtitle_hotkey();
        return Ok(());
    }
    let vk = hotkey::code_to_vk(&settings.key_code)
        .ok_or_else(|| format!("不支持的按键：{}", settings.key_code))?;
    hotkey::set_subtitle_hotkey(vk, subtitle_shortcut_mods(settings))
}

pub(crate) const AUTOSTART_ARG: &str = "--autostarted";

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub(crate) struct StartupSettings {
    #[serde(default)]
    pub(crate) silent_start: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StartupStatus {
    pub(crate) autostart: bool,
    pub(crate) silent_start: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionStatus {
    pub(crate) default_asr_provider: String,
}

#[derive(Clone)]
pub(crate) struct AsrStreamHandle {
    pub(crate) tx: tokio::sync::mpsc::UnboundedSender<AsrStreamInput>,
}

pub(crate) enum AsrStreamInput {
    RawF32(Vec<f32>),
    Finish,
    Stop,
}

#[derive(Serialize)]
pub(crate) struct AsrStreamStartResponse {
    pub(crate) session_id: String,
}

pub(crate) fn decode_f32_base64(input: &str) -> Result<Vec<f32>, String> {
    let bytes = STANDARD
        .decode(input.trim())
        .map_err(|e| format!("invalid base64 f32 audio: {e}"))?;
    if bytes.len() % 4 != 0 {
        return Err(format!(
            "invalid f32 audio byte length: {} is not divisible by 4",
            bytes.len()
        ));
    }
    Ok(bytes
        .chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect())
}
