//! 开发者模式下可选：把每次识别会话的麦克风原始输入和处理后送去识别的 16k 音频存成 WAV，
//! 用真实录音离线复现识别问题、对比处理参数。
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Once;

use crate::audio_wav::{write_mono_pcm16, Quantization};

/// 只保留最近这么多次会话（每次两个文件）。
const KEEP_SESSIONS: usize = 40;
/// 每次会话最多录这么长。字幕这类长会话也走同一条链路，录音全攒在内存里直到结束才写盘。
const MAX_SECONDS: usize = 10 * 60;
const ENABLED_MARKER: &str = "enabled";

static ENABLED: AtomicBool = AtomicBool::new(false);
static LOAD: Once = Once::new();

pub(crate) fn dir() -> PathBuf {
    crate::application::dev_trace::traces_dir().join("audio")
}

/// 开关存成目录里的标记文件，重启后仍然有效；只在开发者模式下生效。
pub(crate) fn enabled() -> bool {
    if !crate::application::diagnostics::developer_mode() {
        return false;
    }
    LOAD.call_once(|| ENABLED.store(dir().join(ENABLED_MARKER).exists(), Ordering::Relaxed));
    ENABLED.load(Ordering::Relaxed)
}

pub(crate) fn set_enabled(value: bool) -> Result<(), String> {
    let marker = dir().join(ENABLED_MARKER);
    if value {
        std::fs::create_dir_all(dir()).map_err(|error| format!("创建录音目录失败：{error}"))?;
        std::fs::write(&marker, b"").map_err(|error| format!("保存录音开关失败：{error}"))?;
    } else if marker.exists() {
        std::fs::remove_file(&marker).map_err(|error| format!("保存录音开关失败：{error}"))?;
    }
    LOAD.call_once(|| {});
    ENABLED.store(value, Ordering::Relaxed);
    Ok(())
}

/// 一次会话的录音；随 `StreamDsp` 一起释放时写盘。
pub(crate) struct DevRecorder {
    started_ms: u128,
    raw_rate: u32,
    raw: Vec<f32>,
    processed: Vec<f32>,
}

impl DevRecorder {
    pub(crate) fn new(raw_rate: u32) -> Self {
        Self {
            started_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |value| value.as_millis()),
            raw_rate,
            raw: Vec::new(),
            processed: Vec::new(),
        }
    }

    pub(crate) fn push_raw(&mut self, samples: &[f32]) {
        let room = (self.raw_rate as usize * MAX_SECONDS).saturating_sub(self.raw.len());
        self.raw.extend_from_slice(&samples[..samples.len().min(room)]);
    }

    pub(crate) fn push_processed(&mut self, pcm16: &[u8]) {
        let room = (super::OUTPUT_RATE as usize * MAX_SECONDS).saturating_sub(self.processed.len());
        self.processed.extend(
            pcm16
                .chunks_exact(2)
                .map(|pair| i16::from_le_bytes([pair[0], pair[1]]) as f32 / 32768.0)
                .take(room),
        );
    }
}

impl Drop for DevRecorder {
    fn drop(&mut self) {
        if self.raw.is_empty() {
            return;
        }
        let dir = dir();
        let result = std::fs::create_dir_all(&dir)
            .and_then(|()| {
                write_mono_pcm16(
                    &dir.join(format!("{}-raw.wav", self.started_ms)),
                    &self.raw,
                    self.raw_rate,
                    Quantization::Round,
                )
            })
            .and_then(|()| {
                write_mono_pcm16(
                    &dir.join(format!("{}-asr.wav", self.started_ms)),
                    &self.processed,
                    super::OUTPUT_RATE,
                    Quantization::Round,
                )
            });
        if let Err(error) = result {
            eprintln!("[dev-audio] 保存录音失败：{error}");
            return;
        }
        prune(&dir);
    }
}

fn prune(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut files = entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "wav"))
        .collect::<Vec<_>>();
    files.sort();
    let excess = files.len().saturating_sub(KEEP_SESSIONS * 2);
    for path in files.into_iter().take(excess) {
        let _ = std::fs::remove_file(path);
    }
}
