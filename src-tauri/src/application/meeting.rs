//! 会议记录：同时录麦克风和系统声音，停止后混成一个 16kHz 单声道 WAV 存进数据目录，
//! 并直接开始录音转写（带说话人分离）。识别由后端发起，停止后马上关掉主窗口也不受影响。
//!
//! 两路边录边写进 `meetings/` 下各自的分轨文件，每隔几秒回写 WAV 头，应用意外退出时
//! 分轨仍是可播放的录音；停止时再按时间对齐相加成一个文件，成功后删除分轨。
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Manager};
use tauri_plugin_opener::OpenerExt;

use crate::application::audio_session::{AudioLease, AudioOwner};
use crate::application::transcription::DEFAULT_TRANSCRIPTION_JOB_KIND as TRANSCRIBE_KIND;
const ALIGN_KIND: &str = "align";
use crate::asr_input::RawAudioReceiver;
use crate::audio_prep::{MonoResampler, TARGET_SAMPLE_RATE};
use crate::audio_storage::BLOCK;
use crate::audio_wav::{Quantization, RecordedWav, WavRecording};
use crate::desktop::backend_mic::{
    attach_backend_mic_raw_inner, pause_backend_mic_inner, release_backend_mic_inner,
    start_backend_mic_inner,
};
use crate::desktop::backend_system_audio::{
    attach_backend_system_audio_raw_inner, pause_backend_system_audio_inner,
    release_backend_system_audio_inner, start_backend_system_audio_inner,
};
use crate::state::{AsrPreroll, RuntimeState};

const RECORDINGS_DIR: &str = "meetings";
/// 一路落后墙钟超过这么多就补静音。系统声音在没有声音播放时不送数据，
/// 不补的话后面的内容会整体提前，和麦克风那一路错开。代价是设备时钟比墙钟慢时，
/// 长会议里每累计落后这么多就会在说话中插进一小段静音。
const GAP_TOLERANCE: usize = TARGET_SAMPLE_RATE as usize / 5;
/// 每写这么多样本回写一次 WAV 头。
const HEADER_SYNC_SAMPLES: usize = TARGET_SAMPLE_RATE as usize * 5;
/// 停止时等每一路写完尾包的上限；设备卡住也不能让之后再也开始不了录制。
const DRAIN_TIMEOUT: Duration = Duration::from_secs(10);
const WAV_HEADER_BYTES: usize = 44;
const BAR_FEED_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Default)]
pub(crate) struct MeetingRuntime {
    recording: Mutex<Option<Recording>>,
    operation: tokio::sync::Mutex<()>,
    /// 已收到停止、正在保存录音。
    stopping: AtomicBool,
    /// 最近一次停止的结果：保存下来的录音，或没能保存的原因；下次开始录制时清空。
    /// 录制可能由屏幕上的录制条停止，主窗口靠它接上随后开始的识别或显示错误。
    last_stop: Mutex<Option<Result<MeetingRecordingFile, String>>>,
}

struct Recording {
    lease: AudioLease,
    started_at_ms: u64,
    output: PathBuf,
    /// 麦克风和系统声音各一路。
    tracks: Vec<Arc<Mutex<Track>>>,
    drains: Vec<tokio::sync::oneshot::Receiver<()>>,
}

struct Track {
    wav: Option<WavRecording>,
    written: usize,
    error: Option<String>,
    /// 上次取走音量之后写入样本的平方和与个数，供录制条显示。
    level_sum: f64,
    level_count: usize,
}

impl Track {
    fn append(&mut self, samples: &[f32]) -> Result<(), String> {
        let wav = self.wav.as_mut().ok_or("会议录音已结束")?;
        wav.append(samples)
            .map_err(|error| format!("写入会议录音失败：{error}"))?;
        self.level_sum += samples.iter().map(|sample| f64::from(sample * sample)).sum::<f64>();
        self.level_count += samples.len();
        let before = self.written;
        self.written += samples.len();
        if before / HEADER_SYNC_SAMPLES != self.written / HEADER_SYNC_SAMPLES {
            wav.sync_header()
                .map_err(|error| format!("写入会议录音失败：{error}"))?;
        }
        Ok(())
    }

    /// 上次调用以来的音量（RMS）；这段时间没有声音送来时为 0。
    fn take_level(&mut self) -> f32 {
        let count = std::mem::take(&mut self.level_count);
        let sum = std::mem::take(&mut self.level_sum);
        if count == 0 {
            0.0
        } else {
            (sum / count as f64).sqrt() as f32
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MeetingStatus {
    recording: bool,
    started_at_ms: Option<u64>,
    /// 某一路中途断开的原因；之后只剩另一路的声音。
    warning: Option<String>,
    stopping: bool,
    last_file: Option<MeetingRecordingFile>,
    last_error: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MeetingRecordingFile {
    path: String,
    warning: Option<String>,
    /// 已开始的识别任务；已有识别在进行或开始失败时为空。
    job_id: Option<String>,
}

impl MeetingRecordingFile {
    /// 没有主窗口时告诉用户的一句结果。
    pub(crate) fn notice(&self) -> String {
        match &self.warning {
            Some(warning) => format!("会议录音已保存。{warning}"),
            None => "会议录音已保存，正在识别".into(),
        }
    }
}

/// 录制条每次刷新要的数据：两路（麦克风、系统声音）的音量和是否已断开。
struct BarFeed {
    levels: [f32; 2],
    lost: [bool; 2],
}

impl MeetingRuntime {
    /// `started_at_ms` 对应的那次录制已经结束时返回 None。
    fn bar_feed(&self, started_at_ms: u64) -> Option<BarFeed> {
        let recording = self.recording.lock().ok()?;
        let recording = recording
            .as_ref()
            .filter(|recording| recording.started_at_ms == started_at_ms)?;
        let mut feed = BarFeed {
            levels: [0.0; 2],
            lost: [false; 2],
        };
        for (index, track) in recording.tracks.iter().enumerate() {
            let mut track = track.lock().ok()?;
            feed.levels[index] = track.take_level();
            feed.lost[index] = track.error.is_some();
        }
        Some(feed)
    }

    /// 数据目录迁移期间持有：录制中拒绝迁移，迁移中也开始不了录制。
    pub(crate) async fn hold_idle_for_migration(&self) -> Result<tokio::sync::MutexGuard<'_, ()>, String> {
        let guard = self.operation.lock().await;
        if self.status()?.recording {
            return Err("正在录制会议，结束后再迁移数据目录".into());
        }
        Ok(guard)
    }

    fn status(&self) -> Result<MeetingStatus, String> {
        let recording = self.recording.lock().map_err(|_| "会议录制状态锁失败")?;
        let warning = recording.as_ref().and_then(|recording| {
            recording
                .tracks
                .iter()
                .find_map(|track| track.lock().ok()?.error.clone())
        });
        let last_stop = self
            .last_stop
            .lock()
            .map_err(|_| "会议录制状态锁失败")?
            .clone();
        Ok(MeetingStatus {
            recording: recording.is_some(),
            started_at_ms: recording.as_ref().map(|value| value.started_at_ms),
            warning,
            stopping: self.stopping.load(Ordering::Acquire),
            last_file: last_stop.as_ref().and_then(|stop| stop.clone().ok()),
            last_error: last_stop.and_then(Result::err),
        })
    }
}

#[tauri::command]
pub(crate) fn get_meeting_status(
    state: tauri::State<'_, RuntimeState>,
) -> Result<MeetingStatus, String> {
    state.meeting_runtime.status()
}

#[tauri::command]
pub(crate) fn open_meeting_recordings(app: AppHandle) -> Result<(), String> {
    let dir = crate::application::data_root::data_subdir(&app, RECORDINGS_DIR)?;
    app.opener()
        .open_path(dir.to_string_lossy(), None::<&str>)
        .map_err(|error| format!("打开会议录音文件夹失败：{error}"))
}

#[tauri::command]
pub(crate) async fn meeting_start(
    app: AppHandle,
    state: tauri::State<'_, RuntimeState>,
) -> Result<MeetingStatus, String> {
    // 先于等锁检查：迁移期间持有同一把锁，等下去按钮会一直转。
    crate::application::data_root::ensure_writable(&app)?;
    let _operation = state.meeting_runtime.operation.lock().await;
    if state.meeting_runtime.status()?.recording {
        return Err("会议正在录制".into());
    }
    let dir = crate::application::data_root::data_subdir(&app, RECORDINGS_DIR)?;
    let lease = state.audio_session.acquire(AudioOwner::Meeting)?;
    let recording = match start_capture(&state, lease.clone(), &dir) {
        Ok(recording) => recording,
        Err(error) => {
            release_devices(&state);
            let _ = state.audio_session.release(&lease);
            return Err(error);
        }
    };
    let started_at_ms = recording.started_at_ms;
    *state
        .meeting_runtime
        .last_stop
        .lock()
        .map_err(|_| "会议录制状态锁失败")? = None;
    *state
        .meeting_runtime
        .recording
        .lock()
        .map_err(|_| "会议录制状态锁失败")? = Some(recording);
    crate::desktop::native_recorder::native_recorder_show(&app);
    spawn_bar_feed(app, started_at_ms);
    state.meeting_runtime.status()
}

/// 录制期间定时把音量和断开状态送给录制条；条上的计时也靠这个节拍刷新。
fn spawn_bar_feed(app: AppHandle, started_at_ms: u64) {
    use crate::desktop::native_recorder::{native_recorder_levels, native_recorder_set_lost};
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(BAR_FEED_INTERVAL);
        // 系统睡眠或运行时卡顿后不补发错过的节拍。
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut lost = [false; 2];
        loop {
            interval.tick().await;
            let Some(feed) = app
                .state::<RuntimeState>()
                .meeting_runtime
                .bar_feed(started_at_ms)
            else {
                break;
            };
            native_recorder_levels(feed.levels[0], feed.levels[1]);
            if feed.lost != lost {
                lost = feed.lost;
                native_recorder_set_lost(lost[0], lost[1]);
            }
        }
    });
}

fn start_capture(state: &RuntimeState, lease: AudioLease, dir: &Path) -> Result<Recording, String> {
    let started_at_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| format!("系统时间无效：{error}"))?
        .as_millis() as u64;
    let stem = format!("meeting-{started_at_ms}");
    let mic_device = crate::application::dictation::preferred_mic_device(state)?;
    let mic = start_backend_mic_inner(mic_device, state)?;
    let system = start_backend_system_audio_inner(None, state)
        .map_err(|error| format!("无法录制电脑播放的声音：{error}"))?;
    // 重采样器和分轨文件都先建好，失败就当场报错，不会显示「录制中」却什么也没录。
    let mut prepared = Vec::new();
    for (name, rate) in [("mic", mic.sample_rate), ("system", system.sample_rate)] {
        let resampler = MonoResampler::new(rate)?;
        let wav = WavRecording::at(
            dir.join(format!("{stem}.{name}.wav")),
            TARGET_SAMPLE_RATE,
            Quantization::Truncate,
        )
        .map_err(|error| format!("创建会议录音文件失败：{error}"))?;
        prepared.push((rate, resampler, wav));
    }
    let (_, mic_receiver) = attach_backend_mic_raw_inner(state, AsrPreroll::Disabled)?;
    let (_, system_receiver) = attach_backend_system_audio_raw_inner(state)?;
    let started = Instant::now();
    let mut tracks = Vec::new();
    let mut drains = Vec::new();
    for (receiver, (rate, resampler, wav)) in
        [mic_receiver, system_receiver].into_iter().zip(prepared)
    {
        let track = Arc::new(Mutex::new(Track {
            wav: Some(wav),
            written: 0,
            error: None,
            level_sum: 0.0,
            level_count: 0,
        }));
        drains.push(spawn_track(receiver, rate, resampler, track.clone(), started));
        tracks.push(track);
    }
    Ok(Recording {
        lease,
        started_at_ms,
        output: dir.join(format!("{stem}.wav")),
        tracks,
        drains,
    })
}

/// 把一路采集持续重采样后写进它的分轨文件；采集结束（设备释放）或出错时退出。
fn spawn_track(
    mut receiver: RawAudioReceiver,
    rate: u32,
    mut resampler: MonoResampler,
    track: Arc<Mutex<Track>>,
    started: Instant,
) -> tokio::sync::oneshot::Receiver<()> {
    let (done, drained) = tokio::sync::oneshot::channel();
    tauri::async_runtime::spawn_blocking(move || {
        let result = (|| -> Result<(), String> {
            let lock = || track.lock().map_err(|_| "会议录音轨道锁失败".to_string());
            let mut append = |chunk: &[f32]| lock()?.append(chunk);
            while let Some(samples) = receiver.blocking_recv()? {
                let elapsed =
                    (started.elapsed().as_secs_f64() * TARGET_SAMPLE_RATE as f64) as usize;
                let incoming = samples.len() * TARGET_SAMPLE_RATE as usize / rate as usize;
                let gap = elapsed
                    .saturating_sub(incoming)
                    .saturating_sub(lock()?.written);
                if gap > GAP_TOLERANCE {
                    let silence = vec![0.0; BLOCK];
                    let mut remaining = gap;
                    while remaining > 0 {
                        let size = remaining.min(BLOCK);
                        append(&silence[..size])?;
                        remaining -= size;
                    }
                }
                resampler.push(&samples, &mut append)?;
            }
            resampler.finish(&mut append).map(|_| ())
        })();
        if let Err(error) = result {
            if let Ok(mut track) = track.lock() {
                track.error = Some(error);
            }
        }
        let _ = done.send(());
    });
    drained
}

fn release_devices(state: &RuntimeState) {
    let _ = pause_backend_mic_inner(state);
    let _ = release_backend_mic_inner(state);
    let _ = pause_backend_system_audio_inner(state);
    let _ = release_backend_system_audio_inner(state);
}

#[tauri::command]
pub(crate) async fn meeting_stop(
    app: AppHandle,
    state: tauri::State<'_, RuntimeState>,
) -> Result<MeetingRecordingFile, String> {
    stop(&app, &state).await
}

/// 主窗口的按钮和录制条的停止按钮共用。识别参数取已保存的「文件」识别设置，
/// 不依赖主窗口还开着。
pub(crate) async fn stop(app: &AppHandle, state: &RuntimeState) -> Result<MeetingRecordingFile, String> {
    let _operation = state.meeting_runtime.operation.lock().await;
    let recording = {
        let mut recording = state
            .meeting_runtime
            .recording
            .lock()
            .map_err(|_| "会议录制状态锁失败")?;
        let recording = recording.take().ok_or("没有正在录制的会议")?;
        // 与取走录制在同一把锁里置位：状态里不会出现「既没在录也没在保存」的瞬间。
        state.meeting_runtime.stopping.store(true, Ordering::Release);
        recording
    };
    crate::desktop::native_recorder::native_recorder_hide();
    let result = save_and_transcribe(app, state, recording).await;
    if let Ok(mut last_stop) = state.meeting_runtime.last_stop.lock() {
        *last_stop = Some(result.clone());
    }
    state.meeting_runtime.stopping.store(false, Ordering::Release);
    result
}

async fn save_and_transcribe(
    app: &AppHandle,
    state: &RuntimeState,
    recording: Recording,
) -> Result<MeetingRecordingFile, String> {
    // 释放设备后发送端关闭，各路写完尾包才退出。
    release_devices(state);
    for drain in recording.drains {
        let _ = tokio::time::timeout(DRAIN_TIMEOUT, drain).await;
    }
    let _ = state.audio_session.release(&recording.lease);
    let mut finished = Vec::new();
    let mut warning = None;
    for track in recording.tracks {
        let mut track = track.lock().map_err(|_| "会议录音轨道锁失败")?;
        warning = warning.or_else(|| track.error.take());
        let Some(wav) = track.wav.take() else {
            continue;
        };
        match wav.finish_written() {
            Ok(file) => finished.push(file),
            Err(error) => {
                warning = warning.or_else(|| Some(format!("保存分轨录音失败：{error}")));
            }
        }
    }
    if finished.iter().all(|file| file.samples == 0) {
        // 两路都是空的：分轨随 `finished` 一起删除，没有可保留的录音。
        return Err(warning.unwrap_or_else(|| "没有录到声音".into()));
    }
    let output = recording.output;
    let target = output.clone();
    let mixed = tauri::async_runtime::spawn_blocking(move || {
        let result = mix_tracks(&target, &finished);
        if result.is_err() {
            // 合成失败时保留分轨文件，录音不丢。
            finished.into_iter().for_each(|file| drop(file.into_path()));
        }
        result
    })
    .await
    .map_err(|error| format!("合成会议录音失败：{error}"))?;
    mixed.map_err(|error| {
        format!(
            "{error}。分轨录音保留在 {}",
            output.parent().unwrap_or(&output).display()
        )
    })?;
    let path = output.to_string_lossy().into_owned();
    let mut warning = warning.map(|error| format!("录制中途有一路声音断开：{error}"));
    // 再开一个会让「文件」页面跟丢正在进行的那个任务，它仍会运行和计费。
    // 文稿对齐和「文件」识别同时进行会互相干扰，前端开始识别时也拦着这种情况。
    let busy = state.transcription_runtime.has_active(TRANSCRIBE_KIND)?
        || state.transcription_runtime.has_active(ALIGN_KIND)?;
    let job_id = if busy {
        warning = Some(match warning {
            Some(previous) => format!("{previous}；已有识别或文稿对齐在进行，没有自动识别"),
            None => "已有识别或文稿对齐在进行，没有自动识别".into(),
        });
        None
    } else {
        let started = match crate::commands::transcription::stored_transcription_params(state) {
            Ok(params) => {
                crate::commands::transcription::transcription_start_inner(
                    app.clone(),
                    state,
                    path.clone(),
                    Some(params),
                    TRANSCRIBE_KIND,
                )
                .await
            }
            Err(error) => Err(error),
        };
        match started {
            Ok(response) => Some(response.job_id),
            Err(error) => {
                let failed = format!("没能开始识别：{error}");
                warning = Some(match warning {
                    Some(previous) => format!("{previous}；{failed}"),
                    None => failed,
                });
                None
            }
        }
    };
    Ok(MeetingRecordingFile {
        path,
        warning,
        job_id,
    })
}

/// 两路按样本位置相加（开始时间相同，静音段已在录制时补齐），短的一路之后按静音算；
/// 相加后超出范围的削顶。
fn mix_tracks(output: &Path, tracks: &[RecordedWav]) -> Result<(), String> {
    let len = tracks.iter().map(|track| track.samples).max().unwrap_or(0);
    if len == 0 {
        return Err("没有录到声音".into());
    }
    let read_error = |error: std::io::Error| format!("读取分轨录音失败：{error}");
    let mut readers = tracks
        .iter()
        .map(|track| {
            let mut reader = BufReader::new(std::fs::File::open(track.path())?);
            reader.read_exact(&mut [0u8; WAV_HEADER_BYTES])?;
            Ok((reader, track.samples))
        })
        .collect::<std::io::Result<Vec<_>>>()
        .map_err(read_error)?;
    let mut mixed = WavRecording::at(
        output.to_path_buf(),
        TARGET_SAMPLE_RATE,
        Quantization::Truncate,
    )
    .map_err(|error| format!("创建会议录音失败：{error}"))?;
    let mut bytes = vec![0u8; BLOCK * 2];
    let mut sum = vec![0f32; BLOCK];
    for start in (0..len).step_by(BLOCK) {
        let size = BLOCK.min(len - start);
        sum[..size].fill(0.0);
        for (reader, samples) in &mut readers {
            let available = samples.saturating_sub(start).min(size);
            reader
                .read_exact(&mut bytes[..available * 2])
                .map_err(read_error)?;
            for (total, pair) in sum.iter_mut().zip(bytes[..available * 2].chunks_exact(2)) {
                *total += i16::from_le_bytes([pair[0], pair[1]]) as f32 / i16::MAX as f32;
            }
        }
        for sample in &mut sum[..size] {
            *sample = sample.clamp(-1.0, 1.0);
        }
        mixed
            .append(&sum[..size])
            .map_err(|error| format!("合成会议录音失败：{error}"))?;
    }
    // 交付成品：去掉删除守卫，文件留在 `meetings/`。
    mixed
        .finish()
        .map(|file| drop(file.into_path()))
        .map_err(|error| format!("合成会议录音失败：{error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path() -> PathBuf {
        std::env::temp_dir().join(format!("sayit-meeting-{}.wav", uuid::Uuid::new_v4()))
    }

    fn recorded(samples: &[f32]) -> RecordedWav {
        let mut wav =
            WavRecording::at(temp_path(), TARGET_SAMPLE_RATE, Quantization::Truncate).unwrap();
        wav.append(samples).unwrap();
        wav.finish().unwrap()
    }

    #[test]
    fn mixing_adds_tracks_and_treats_the_end_of_the_shorter_one_as_silence() {
        let output = temp_path();
        mix_tracks(&output, &[recorded(&[0.25; 3]), recorded(&[0.9])]).unwrap();

        let bytes = std::fs::read(&output).unwrap();
        let samples: Vec<i16> = bytes[WAV_HEADER_BYTES..]
            .chunks(2)
            .map(|pair| i16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        assert_eq!(samples.len(), 3);
        assert_eq!(samples[0], i16::MAX, "相加超出范围应削顶");
        assert!((samples[1] as f32 / i16::MAX as f32 - 0.25).abs() < 0.001);
        assert!((samples[2] as f32 / i16::MAX as f32 - 0.25).abs() < 0.001);
        std::fs::remove_file(&output).unwrap();
    }

    #[test]
    fn an_empty_recording_is_not_saved() {
        let output = temp_path();
        assert!(mix_tracks(&output, &[recorded(&[]), recorded(&[])]).is_err());
        assert!(!output.exists());
    }
}
