use crate::obs_overlay::{ObsOverlayRuntime, ObsOverlaySettings};
use crate::prelude::*;
use std::sync::atomic::AtomicU64;

#[derive(Default)]
pub(crate) struct RuntimeState {
    pub(crate) snapshot_revision: AtomicU64,
    pub(crate) app_settings: Mutex<crate::application::settings::AppSettings>,
    pub(crate) providers: Mutex<ProviderSettings>,
    pub(crate) plugin_registry: Mutex<crate::providers::plugin::PluginRegistry>,
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
    "CapsLock".to_string()
}

pub(crate) fn default_inject_method() -> String {
    "paste".to_string()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
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
        }
    }
}

pub(crate) fn dictation_mods(settings: &DictationSettings) -> u8 {
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

/// 应用语音输入热键；key_code 为空表示未设置，直接清除即可。
pub(crate) fn apply_dictation_hotkey(settings: &DictationSettings) -> Result<(), String> {
    if settings.key_code.trim().is_empty() {
        hotkey::clear_hotkey();
        return Ok(());
    }
    let vk = hotkey::code_to_vk(&settings.key_code)
        .ok_or_else(|| format!("不支持的按键：{}", settings.key_code))?;
    hotkey::set_hotkey(vk, dictation_mods(settings), settings.press_hold_mode);
    Ok(())
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
