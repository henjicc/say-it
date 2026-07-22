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
    pub(crate) audio_lab_lease: Mutex<Option<crate::application::audio_session::AudioLease>>,
    pub(crate) main_window_lifecycle:
        Mutex<crate::application::window_lifecycle::MainWindowLifecycle>,
    /// 实时字幕"系统音频"来源用的 loopback 采集状态，和麦克风共用同一套结构体但各自独立。
    pub(crate) backend_system_audio: Arc<Mutex<BackendMicState>>,
    pub(crate) main_window_placement: Mutex<Option<MainWindowPlacement>>,
    pub(crate) obs_overlay_settings: Mutex<ObsOverlaySettings>,
    pub(crate) obs_overlay_runtime: ObsOverlayRuntime,
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
    if cfg!(target_os = "macos") {
        "Space".to_string()
    } else {
        "CapsLock".to_string()
    }
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
            shift: cfg!(target_os = "macos"),
            alt: false,
            meta: cfg!(target_os = "macos"),
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
    hotkey::set_subtitle_hotkey(vk, subtitle_shortcut_mods(settings));
    Ok(())
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
