//! 听写提示音的原生播放。
//!
//! 提示音要和指示器同一时刻出现：在 Rust 里直接播放，不经过「派发事件 → WebView → Web Audio」
//! 这条有延迟的链路，也不依赖主窗口是否还开着。
//! 输出流打开后一直输出静音，提示音只往里写样本：每次提示都临时打开、关闭输出设备
//! （系统 PlaySound 和临时 CPAL 流都是这样）会在部分音箱上于开始和结束处发出「呲啦」声。
//! 常驻会让系统认为一直有音频在播放（阻止空闲自动睡眠、蓝牙设备不待机），所以保持多久由用户设置；
//! 不保持时只在提示音前后打开，播完稍后自动关闭。
//! 非 Windows 平台和解析不了的自定义文件返回 false，由调用方交给 WebView 播放
//! `ui/public/cues/` 下的同一批文件；交给输出流后不再回退，设备打不开时表现为静音。

use crate::prelude::*;

#[cfg(windows)]
macro_rules! builtin {
    ($($kind:literal),* $(,)?) => {
        &[$(($kind, include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../ui/public/cues/", $kind, ".wav")) as &[u8])),*]
    };
}

/// 内置音色（只有 Windows 原生播放用到）；文件由 `scripts/synth-cues.py` 生成，
/// 前端音色目录 `ui/src/lib/cues.ts` 使用同一批 ID。
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

/// 已交给常驻输出流播放返回 true；返回 false 时调用方应改由 WebView 播放。
#[cfg(windows)]
pub(crate) fn play_native_cue(app: &tauri::AppHandle, kind: &str, which: &str) -> bool {
    let sound = if let Some((_, wav)) = BUILTIN_CUES.iter().find(|(id, _)| *id == kind) {
        pcm16_wav_to_mono(wav)
    } else if kind == "custom" {
        custom_cue_path(app, which)
            .and_then(|path| std::fs::read(path).ok())
            .and_then(|bytes| pcm16_wav_to_mono(&bytes))
    } else {
        None
    };
    match sound {
        Some((samples, rate)) => output::play(samples, rate),
        None => false,
    }
}

#[cfg(not(windows))]
pub(crate) fn play_native_cue(_app: &tauri::AppHandle, _kind: &str, _which: &str) -> bool {
    false
}

/// 提示音输出流的保持策略。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CueHold {
    /// 不保持：提示音播完稍后即关闭。
    Off,
    /// 最后一次提示音后再保持这么久。
    For(Duration),
    /// 一直保持到关闭提示音或退出应用。
    Forever,
}

/// 按设置打开或关闭提示音输出流；`enabled` 为 false 时立即关闭。
#[cfg(windows)]
pub(crate) fn configure_cue_output(enabled: bool, hold: CueHold) {
    output::configure(enabled, hold);
}

#[cfg(not(windows))]
pub(crate) fn configure_cue_output(_enabled: bool, _hold: CueHold) {}

/// 听写开始时提前打开（保持模式下），让开始音不必等设备。
#[cfg(windows)]
pub(crate) fn warm_cue_output() {
    output::warm();
}

#[cfg(not(windows))]
pub(crate) fn warm_cue_output() {}

#[cfg(windows)]
fn custom_cue_path(app: &tauri::AppHandle, which: &str) -> Option<PathBuf> {
    let state = app.state::<crate::state::RuntimeState>();
    let settings = state.app_settings.lock().ok()?;
    let file = if which == "start" {
        settings.custom_cue_start.clone()
    } else {
        settings.custom_cue_end.clone()
    }?;
    drop(settings);
    Some(crate::application::data_root::data_root(app).ok()?.join(file.relative_path))
}

/// 解析 16 位 PCM WAV，多声道取平均；其他编码（mp3、24 位 WAV 等）返回 None。
#[cfg(windows)]
fn pcm16_wav_to_mono(bytes: &[u8]) -> Option<(Vec<f32>, u32)> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return None;
    }
    let u16_at = |i: usize| u16::from_le_bytes([bytes[i], bytes[i + 1]]);
    let u32_at = |i: usize| u32::from_le_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]);
    let (mut format, mut pos) = (None, 12);
    while pos + 8 <= bytes.len() {
        let size = u32_at(pos + 4) as usize;
        let body = pos + 8;
        let end = body.checked_add(size)?.min(bytes.len());
        match &bytes[pos..pos + 4] {
            b"fmt " if size >= 16 && end >= body + 16 => {
                format = Some((u16_at(body), u16_at(body + 2), u32_at(body + 4), u16_at(body + 14)))
            }
            b"data" => {
                let (tag, channels, rate, bits) = format?;
                if tag != 1 || bits != 16 || channels == 0 || rate == 0 {
                    return None;
                }
                let channels = channels as usize;
                let samples = bytes[body..end]
                    .chunks_exact(2 * channels)
                    .map(|frame| {
                        frame.chunks_exact(2).map(|s| i16::from_le_bytes([s[0], s[1]]) as f32).sum::<f32>()
                            / (channels as f32 * 32768.0)
                    })
                    .collect();
                return Some((samples, rate));
            }
            _ => {}
        }
        pos = end + (size & 1);
    }
    None
}

#[cfg(windows)]
mod output {
    use super::CueHold;
    use crate::dlog;
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use cpal::{FromSample, SizedSample};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
    use std::sync::{Arc, Mutex, OnceLock};
    use std::time::{Duration, Instant};

    /// 提示音播完后至少再留这么久才关闭，覆盖设备缓冲区里尚未放完的尾音。
    const MIN_TAIL: Duration = Duration::from_secs(2);

    /// 正在播放的提示音：设备采样率下的单声道样本与已读位置。几声重叠时相加，不硬切前一声。
    type Playing = Arc<Mutex<Vec<(Vec<f32>, usize)>>>;

    enum Command {
        Configure(bool, CueHold),
        Warm,
        Play(Vec<f32>, u32),
    }

    struct Output {
        _stream: cpal::Stream,
        device: String,
        rate: u32,
        playing: Playing,
        failed: Arc<AtomicBool>,
    }

    fn sender() -> Option<&'static Sender<Command>> {
        static SENDER: OnceLock<Option<Sender<Command>>> = OnceLock::new();
        SENDER
            .get_or_init(|| {
                let (tx, rx) = channel();
                // cpal::Stream 不能跨线程移动，输出流整个生命周期都留在这条线程上。
                match std::thread::Builder::new().name("cue-output".into()).spawn(move || run(rx)) {
                    Ok(_) => Some(tx),
                    Err(error) => {
                        dlog!("[cue] 创建提示音输出线程失败：{error}");
                        None
                    }
                }
            })
            .as_ref()
    }

    fn send(command: Command) -> bool {
        sender().is_some_and(|tx| tx.send(command).is_ok())
    }

    pub(super) fn configure(enabled: bool, hold: CueHold) {
        send(Command::Configure(enabled, hold));
    }

    pub(super) fn warm() {
        send(Command::Warm);
    }

    pub(super) fn play(samples: Vec<f32>, rate: u32) -> bool {
        send(Command::Play(samples, rate))
    }

    fn run(rx: Receiver<Command>) {
        let mut output: Option<Output> = None;
        let mut hold = CueHold::Off;
        // 到点关闭输出流；None 表示没打开或一直保持。
        let mut close_at: Option<Instant> = None;
        loop {
            let command = match close_at {
                Some(at) => match rx.recv_timeout(at.saturating_duration_since(Instant::now())) {
                    Ok(command) => command,
                    Err(RecvTimeoutError::Timeout) => {
                        output = None;
                        close_at = None;
                        continue;
                    }
                    Err(RecvTimeoutError::Disconnected) => return,
                },
                None => match rx.recv() {
                    Ok(command) => command,
                    Err(_) => return,
                },
            };
            match command {
                Command::Configure(enabled, next) => {
                    hold = next;
                    if !enabled || hold == CueHold::Off {
                        output = None;
                        close_at = None;
                    } else {
                        ensure_output(&mut output);
                        close_at = deadline(hold, Duration::ZERO);
                    }
                }
                Command::Warm => {
                    if hold != CueHold::Off {
                        ensure_output(&mut output);
                        close_at = deadline(hold, Duration::ZERO);
                    }
                }
                Command::Play(samples, rate) => {
                    // 查默认输出设备每次要三四十毫秒，提示音会比胶囊晚；输出流完好时直接播放，
                    // 换设备的检查留给听写开始时的 Warm。
                    if !output.as_ref().is_some_and(|out| !out.failed.load(Ordering::Relaxed)) {
                        ensure_output(&mut output);
                    }
                    let Some(out) = &output else { continue };
                    let samples = with_fades(resample(&samples, rate, out.rate), out.rate);
                    let length = Duration::from_secs_f64(samples.len() as f64 / out.rate as f64);
                    if let Ok(mut playing) = out.playing.lock() {
                        playing.retain(|(s, pos)| *pos < s.len());
                        playing.push((samples, 0));
                    }
                    close_at = deadline(hold, length);
                }
            }
        }
    }

    fn deadline(hold: CueHold, sound: Duration) -> Option<Instant> {
        let end = Instant::now() + sound;
        match hold {
            CueHold::Off => Some(end + MIN_TAIL),
            CueHold::For(duration) => Some(end + duration.max(MIN_TAIL)),
            CueHold::Forever => None,
        }
    }

    /// 没有输出流、流出过错或默认输出设备变了时重新打开。
    fn ensure_output(output: &mut Option<Output>) {
        let Some(device) = cpal::default_host().default_output_device() else {
            *output = None;
            return;
        };
        let name = device.name().unwrap_or_default();
        if let Some(out) = output {
            if !out.failed.load(Ordering::Relaxed) && out.device == name {
                return;
            }
        }
        *output = None;
        match open(&device, name) {
            Ok(out) => *output = Some(out),
            Err(error) => dlog!("[cue] 打开提示音输出流失败：{error}"),
        }
    }

    fn open(device: &cpal::Device, name: String) -> Result<Output, String> {
        let config = device.default_output_config().map_err(|e| e.to_string())?;
        let stream_config: cpal::StreamConfig = config.clone().into();
        let channels = stream_config.channels.max(1) as usize;
        let playing: Playing = Arc::new(Mutex::new(Vec::new()));
        let failed = Arc::new(AtomicBool::new(false));
        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => build::<f32>(device, &stream_config, channels, &playing, &failed),
            cpal::SampleFormat::I16 => build::<i16>(device, &stream_config, channels, &playing, &failed),
            cpal::SampleFormat::U16 => build::<u16>(device, &stream_config, channels, &playing, &failed),
            other => return Err(format!("不支持的输出采样格式：{other:?}")),
        }?;
        stream.play().map_err(|e| e.to_string())?;
        Ok(Output { _stream: stream, device: name, rate: stream_config.sample_rate.0, playing, failed })
    }

    fn build<T: SizedSample + FromSample<f32>>(
        device: &cpal::Device,
        config: &cpal::StreamConfig,
        channels: usize,
        playing: &Playing,
        failed: &Arc<AtomicBool>,
    ) -> Result<cpal::Stream, String> {
        let playing = playing.clone();
        let failed = failed.clone();
        device
            .build_output_stream(
                config,
                move |data: &mut [T], _| {
                    // 音频回调里不能等锁：拿不到就输出这一块静音。
                    let mut guard = playing.try_lock().ok();
                    for frame in data.chunks_mut(channels) {
                        let mut value = 0.0;
                        for (samples, pos) in guard.iter_mut().flat_map(|g| g.iter_mut()) {
                            if *pos < samples.len() {
                                value += samples[*pos];
                                *pos += 1;
                            }
                        }
                        frame.fill(T::from_sample(value.clamp(-1.0, 1.0)));
                    }
                },
                move |error| {
                    dlog!("[cue] 提示音输出流出错：{error}");
                    failed.store(true, Ordering::Relaxed);
                },
                None,
            )
            .map_err(|e| e.to_string())
    }

    fn resample(input: &[f32], from: u32, to: u32) -> Vec<f32> {
        if from == to || input.is_empty() {
            return input.to_vec();
        }
        let ratio = from as f64 / to as f64;
        let len = (input.len() as f64 / ratio) as usize;
        (0..len)
            .map(|k| {
                let pos = k as f64 * ratio;
                let i = pos as usize;
                let frac = (pos - i as f64) as f32;
                let a = input[i.min(input.len() - 1)];
                let b = input[(i + 1).min(input.len() - 1)];
                a + (b - a) * frac
            })
            .collect()
    }

    /// 头尾各 3ms 淡入淡出：自定义文件常常不是从零起止，硬切也会「咔」一声。
    fn with_fades(mut samples: Vec<f32>, rate: u32) -> Vec<f32> {
        let len = samples.len();
        let n = (rate as usize * 3 / 1000).min(len / 2);
        for i in 0..n {
            let gain = i as f32 / n as f32;
            samples[i] *= gain;
            samples[len - 1 - i] *= gain;
        }
        samples
    }
}

#[cfg(all(test, windows))]
mod tests {
    /// 前端音色目录与后端原生播放用同一批文件；这里保证两边的 ID 不会漏改，且都能解析。
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
        for (kind, wav) in super::BUILTIN_CUES {
            assert!(super::pcm16_wav_to_mono(wav).is_some(), "{kind}");
        }
    }
}
