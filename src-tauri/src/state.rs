use crate::prelude::*;

#[derive(Default)]
pub(crate) struct RuntimeState {
    pub(crate) providers: Mutex<ProviderSettings>,
    pub(crate) asr_streams: Arc<Mutex<HashMap<String, AsrStreamHandle>>>,
    pub(crate) dictation: Mutex<DictationSettings>,
    pub(crate) startup: Mutex<StartupSettings>,
    pub(crate) backend_mic: Arc<Mutex<BackendMicState>>,
    pub(crate) main_window_placement: Mutex<Option<MainWindowPlacement>>,
}

#[derive(Default)]
pub(crate) struct BackendMicState {
    pub(crate) worker: Option<std::sync::mpsc::Sender<BackendMicCommand>>,
    pub(crate) sample_rate: u32,
    pub(crate) channels: usize,
    pub(crate) session_id: Option<String>,
    pub(crate) tx: Option<tokio::sync::mpsc::UnboundedSender<AsrStreamInput>>,
    pub(crate) pending: VecDeque<Vec<f32>>,
    pub(crate) buffer: Vec<f32>,
    pub(crate) chunk_count: u64,
}

pub(crate) enum BackendMicCommand {
    Attach {
        session_id: String,
        tx: tokio::sync::mpsc::UnboundedSender<AsrStreamInput>,
        reply: std::sync::mpsc::Sender<Result<BackendMicAttachResponse, String>>,
    },
    Pause {
        reply: std::sync::mpsc::Sender<Result<usize, String>>,
    },
    Stop,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BackendMicStartResponse {
    pub(crate) sample_rate: u32,
    pub(crate) channels: usize,
    pub(crate) reused: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BackendMicAttachResponse {
    pub(crate) flushed_chunks: usize,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct MainWindowPlacement {
    pub(crate) position: tauri::PhysicalPosition<i32>,
    pub(crate) size: tauri::PhysicalSize<u32>,
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

pub(crate) fn apply_dictation_hotkey(settings: &DictationSettings) -> Result<(), String> {
    let vk = hotkey::code_to_vk(&settings.key_code)
        .ok_or_else(|| format!("不支持的按键：{}", settings.key_code))?;
    hotkey::set_hotkey(vk, dictation_mods(settings));
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

#[derive(Serialize)]
pub(crate) struct AsrResponse {
    pub(crate) text: String,
    pub(crate) partials: Vec<String>,
    pub(crate) events: Vec<Value>,
}

#[derive(Clone)]
pub(crate) struct AsrStreamHandle {
    pub(crate) tx: tokio::sync::mpsc::UnboundedSender<AsrStreamInput>,
}

pub(crate) enum AsrStreamInput {
    Audio(Vec<u8>),
    RawF32(Vec<f32>),
    Finish,
    Stop,
}

#[derive(Serialize)]
pub(crate) struct AsrStreamStartResponse {
    pub(crate) session_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AudioProcessRequest {
    pub(crate) samples_base64: String,
    pub(crate) sample_rate: u32,
    pub(crate) params: DspParams,
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
