//! 听写提示音的原生播放。
//!
//! 提示音要和指示器同一时刻出现：在 Rust 里直接交给系统播放，不经过「派发事件 → WebView → Web Audio」
//! 这条有延迟的链路，也不依赖主窗口是否还开着。用 Windows 自带的 PlaySound 而不是自建 CPAL 输出流：
//! 每次临时创建、销毁 CPAL 流会在部分扬声器驱动上破音。
//! 非 Windows 平台和非 WAV 的自定义文件返回 false，由调用方交给 WebView 播放
//! `ui/public/cues/` 下的同一批文件。交给系统后（异步播放）不再回退：设备缺失等失败表现为静音。

#[cfg(windows)]
use crate::prelude::*;

#[cfg(windows)]
macro_rules! builtin {
    ($($kind:literal),* $(,)?) => {
        &[$(($kind, include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../ui/public/cues/", $kind, ".wav")) as &[u8])),*]
    };
}

/// 内置音色（只有 Windows 原生播放用到）；文件由 `scripts/synth-cues.py` 生成，前端音色目录 `ui/src/lib/cues.ts` 使用同一批 ID。
#[cfg(windows)]
const BUILTIN_CUES: &[(&str, &[u8])] = builtin![
    "beep-up", "beep-down", "beep-double",
    "drop-up", "drop-down",
    "marimba-up", "marimba-down",
    "bell-high", "bell-low",
    "tick-up", "tick-down",
    "soft-high", "soft-low",
    "arp-up", "arp-down",
    "marimba-low-up", "marimba-low-down",
];

/// 已交给系统播放返回 true；返回 false 时调用方应改由 WebView 播放。
#[cfg(windows)]
pub(crate) fn play_native_cue(app: &tauri::AppHandle, kind: &str, which: &str) -> bool {
    if let Some((_, wav)) = BUILTIN_CUES.iter().find(|(id, _)| *id == kind) {
        return imp::play_memory(wav);
    }
    if kind == "custom" {
        if let Some(path) = custom_wav_path(app, which) {
            return imp::play_file(&path);
        }
    }
    false
}

#[cfg(not(windows))]
pub(crate) fn play_native_cue(_app: &tauri::AppHandle, _kind: &str, _which: &str) -> bool {
    false
}

/// 只有 RIFF/WAVE 文件能交给系统直接播放；其他格式（mp3、ogg 等）返回 None。
#[cfg(windows)]
fn custom_wav_path(app: &tauri::AppHandle, which: &str) -> Option<PathBuf> {
    let state = app.state::<crate::state::RuntimeState>();
    let settings = state.app_settings.lock().ok()?;
    let file = if which == "start" {
        settings.custom_cue_start.clone()
    } else {
        settings.custom_cue_end.clone()
    }?;
    drop(settings);
    let path = crate::application::data_root::data_root(app).ok()?.join(file.relative_path);
    let mut header = [0u8; 12];
    std::io::Read::read_exact(&mut std::fs::File::open(&path).ok()?, &mut header).ok()?;
    (&header[0..4] == b"RIFF" && &header[8..12] == b"WAVE").then_some(path)
}

#[cfg(windows)]
mod imp {
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::HMODULE;
    use windows::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_FILENAME, SND_MEMORY, SND_NODEFAULT};

    pub(super) fn play_memory(wav: &'static [u8]) -> bool {
        // SND_ASYNC 下系统在播放期间持续读这块内存，所以只能传 'static 数据。
        unsafe {
            PlaySoundW(
                PCWSTR(wav.as_ptr().cast()),
                HMODULE::default(),
                SND_MEMORY | SND_ASYNC | SND_NODEFAULT,
            )
            .as_bool()
        }
    }

    pub(super) fn play_file(path: &Path) -> bool {
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        unsafe {
            PlaySoundW(
                PCWSTR(wide.as_ptr()),
                HMODULE::default(),
                SND_FILENAME | SND_ASYNC | SND_NODEFAULT,
            )
            .as_bool()
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    /// 前端音色目录与后端原生播放用同一批文件；这里保证两边的 ID 不会漏改。
    #[test]
    fn builtin_cues_match_bundled_files() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../ui/public/cues");
        let mut files: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().is_some_and(|ext| ext == "wav"))
            .map(|p| p.file_stem().unwrap().to_string_lossy().into_owned())
            .collect();
        let mut ids: Vec<String> = super::BUILTIN_CUES.iter().map(|(id, _)| id.to_string()).collect();
        files.sort();
        ids.sort();
        assert_eq!(ids, files);
    }
}
