//! 音频调校会话：原始 PCM、离线 DSP 和波形摘要只驻留在 Rust。
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use serde_json::json;
use tauri::{Emitter, Manager};

use crate::application::contract::{
    next_revision, DomainEventEnvelope, DomainRunState, DomainSnapshot,
};
use crate::audio_dsp::{offline, DspParams};
use crate::audio_storage::AudioBuffer;
use crate::temporary_audio::TemporaryFile;

const WAVE_POINTS: usize = 860;

#[cfg(all(test, windows))]
mod performance_tests;
#[cfg(feature = "performance-acceptance")]
mod acceptance;

#[derive(Default)]
pub(crate) struct AudioLabRuntime {
    state: Mutex<AudioLabState>,
    operation: tokio::sync::Mutex<()>,
    processing_gate: Arc<tokio::sync::Mutex<()>>,
    processing_revision: AtomicU64,
    // 当前播放器可能继续发起分段读取；调参或重新录音不能提前删除它正在使用的文件。
    playback: Mutex<Option<Arc<TemporaryFile>>>,
}

#[derive(Default)]
struct AudioLabState {
    epoch: u64,
    drain: Option<tokio::sync::oneshot::Receiver<Result<(), String>>>,
    recording: bool,
    stopping: bool,
    sample_rate: u32,
    raw: AudioBuffer,
    processed: AudioBuffer,
    raw_preview: Option<Arc<TemporaryFile>>,
    processed_preview: Option<Arc<TemporaryFile>>,
    stats: Option<AudioLabStats>,
    error: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AudioLabStats {
    pub(crate) in_lufs: f32,
    pub(crate) out_lufs: f32,
    pub(crate) in_peak_db: f32,
    pub(crate) out_peak_db: f32,
    pub(crate) clipped_samples: usize,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AudioLabSnapshot {
    pub(crate) recording: bool,
    pub(crate) sample_rate: u32,
    pub(crate) duration_ms: u64,
    pub(crate) raw_waveform: Vec<[f32; 2]>,
    pub(crate) processed_waveform: Vec<[f32; 2]>,
    pub(crate) stats: Option<AudioLabStats>,
    pub(crate) error: Option<String>,
}

impl AudioLabRuntime {
    #[cfg(windows)]
    pub(crate) fn is_idle_for_reclaim(&self) -> bool {
        self.processing_gate.try_lock().is_ok()
            && self.operation.try_lock().is_ok()
            && self.state.lock().is_ok_and(|state| !state.recording)
    }

    pub(crate) fn is_recording(&self) -> Result<bool, String> {
        self.state
            .lock()
            .map(|state| state.recording)
            .map_err(|_| "音频调校状态锁失败".into())
    }
    pub(crate) fn begin(&self, sample_rate: u32) -> Result<u64, String> {
        let mut state = self.state.lock().map_err(|_| "音频调校状态锁失败")?;
        if state.recording {
            return Err("音频调校正在录音".into());
        }
        let epoch = state.epoch.wrapping_add(1);
        self.processing_revision.fetch_add(1, Ordering::AcqRel);
        *state = AudioLabState {
            epoch,
            recording: true,
            sample_rate,
            ..Default::default()
        };
        Ok(epoch)
    }
    #[cfg(test)]
    pub(crate) fn append(&self, samples: &[f32]) {
        let epoch = self.state.lock().unwrap().epoch;
        self.append_for(epoch, samples).unwrap();
    }
    fn abort(&self) {
        if let Ok(mut state) = self.state.lock() {
            let epoch = state.epoch.wrapping_add(1);
            *state = AudioLabState {
                epoch,
                ..Default::default()
            };
        }
    }
    fn append_for(&self, epoch: u64, samples: &[f32]) -> Result<(), String> {
        let mut state = self.state.lock().map_err(|_| "音频调校状态锁失败")?;
        if state.epoch != epoch || !state.recording {
            return Err("音频调校会话已结束".into());
        }
        state
            .raw
            .append(samples)
            .map_err(|e| format!("暂存录音失败：{e}"))?;
        state.raw_preview = None;
        Ok(())
    }
    fn fail_for(&self, epoch: u64, error: String) -> bool {
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        if state.epoch != epoch || !state.recording {
            return false;
        }
        state.recording = false;
        state.error = Some(error);
        state.drain.take();
        true
    }
    fn register_drain(
        &self,
        epoch: u64,
        drain: tokio::sync::oneshot::Receiver<Result<(), String>>,
    ) -> Result<(), String> {
        let mut state = self.state.lock().map_err(|_| "音频调校状态锁失败")?;
        if state.epoch != epoch || !state.recording {
            return Err("音频调校会话已结束".into());
        }
        state.drain = Some(drain);
        Ok(())
    }
    fn request_stop(&self) -> Result<u64, String> {
        let mut state = self.state.lock().map_err(|_| "音频调校状态锁失败")?;
        state.stopping = true;
        Ok(state.epoch)
    }
    fn finish_input(&self, epoch: u64) -> Result<(), String> {
        let state = self.state.lock().map_err(|_| "音频调校状态锁失败")?;
        if state.epoch != epoch {
            return Err("音频调校会话已结束".into());
        }
        if !state.stopping {
            return Err("音频采集已意外停止".into());
        }
        Ok(())
    }
    async fn drain_capture(&self) -> Result<(), String> {
        let (epoch, drain) = {
            let mut state = self.state.lock().map_err(|_| "音频调校状态锁失败")?;
            (state.epoch, state.drain.take())
        };
        let result = match drain {
            Some(drain) => {
                match tokio::time::timeout(std::time::Duration::from_secs(10), drain).await {
                    Ok(Ok(result)) => result,
                    Ok(Err(_)) => Err("音频调校消费线程提前退出".into()),
                    Err(_) => Err("音频调校尾部处理超时，请重新录音".into()),
                }
            }
            None => Err("音频调校消费线程未注册".into()),
        };
        if let Err(error) = &result {
            self.fail_for(epoch, error.clone());
        }
        result
    }
    pub(crate) fn stop(&self) -> Result<(), String> {
        let mut state = self.state.lock().map_err(|_| "音频调校状态锁失败")?;
        state.recording = false;
        if state.raw.is_empty() {
            return Err("未录到音频".into());
        }
        Ok(())
    }
    #[cfg(test)]
    pub(crate) fn reprocess(&self, params: DspParams) -> Result<AudioLabSnapshot, String> {
        let revision = self.processing_revision.fetch_add(1, Ordering::AcqRel) + 1;
        self.reprocess_at(params, revision)
    }
    fn reprocess_at(&self, params: DspParams, revision: u64) -> Result<AudioLabSnapshot, String> {
        let (epoch, input, rate) = {
            let state = self.state.lock().map_err(|_| "音频调校状态锁失败")?;
            if state.raw.is_empty() {
                return Err("请先录制音频".into());
            }
            (state.epoch, state.raw.snapshot(), state.sample_rate)
        };
        let result = offline::process_cancellable(&input, rate, &params, || {
            self.processing_revision.load(Ordering::Acquire) != revision
        })?;
        let mut state = self.state.lock().map_err(|_| "音频调校状态锁失败")?;
        if state.epoch != epoch || self.processing_revision.load(Ordering::Acquire) != revision {
            return Err("音频处理已被新任务替换".into());
        }
        state.processed = result.processed;
        state.processed_preview = None;
        state.stats = Some(AudioLabStats {
            in_lufs: result.in_lufs,
            out_lufs: result.out_lufs,
            in_peak_db: result.in_peak_db,
            out_peak_db: result.out_peak_db,
            clipped_samples: result.clipped_samples,
        });
        snapshot(&state)
    }
    pub(crate) fn snapshot(&self) -> Result<AudioLabSnapshot, String> {
        let state = self.state.lock().map_err(|_| "音频调校状态锁失败")?;
        snapshot(&state)
    }
    pub(crate) fn domain_snapshot(&self) -> DomainSnapshot {
        match self.state.lock() {
            Ok(state) => DomainSnapshot {
                state: if state.error.is_some() {
                    DomainRunState::Failed
                } else if state.recording {
                    DomainRunState::Running
                } else {
                    DomainRunState::Idle
                },
                session_id: None,
            },
            Err(_) => DomainSnapshot {
                state: DomainRunState::Failed,
                session_id: None,
            },
        }
    }
    pub(crate) fn write_wav(&self, processed: bool) -> Result<String, String> {
        let mut state = self.state.lock().map_err(|_| "音频调校状态锁失败")?;
        let existing = if processed {
            &state.processed_preview
        } else {
            &state.raw_preview
        };
        if let Some(file) = existing {
            *self.playback.lock().map_err(|_| "试听状态锁失败")? = Some(file.clone());
            return Ok(file.readable_path().to_string_lossy().into_owned());
        }
        let samples = if processed {
            &state.processed
        } else {
            &state.raw
        };
        if samples.is_empty() {
            return Err("没有可播放的音频".into());
        }
        let rate = if processed { 48_000 } else { state.sample_rate };
        let file =
            TemporaryFile::create_readable().map_err(|e| format!("创建试听文件失败：{e}"))?;
        {
            let mut writer = std::io::BufWriter::with_capacity(64 * 1024, file.writer());
            crate::audio_wav::write_audio_buffer(&mut writer, samples, rate)
                .map_err(|e| format!("写入试听文件失败：{e}"))?;
        }
        let path = file
            .readable_path()
            .to_str()
            .ok_or("试听文件路径无效")?
            .to_owned();
        let file = Arc::new(file);
        *self.playback.lock().map_err(|_| "试听状态锁失败")? = Some(file.clone());
        if processed {
            state.processed_preview = Some(file);
        } else {
            state.raw_preview = Some(file);
        }
        Ok(path)
    }
}

#[tauri::command]
pub(crate) async fn audio_lab_start(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::state::RuntimeState>,
    device_name: Option<String>,
) -> Result<AudioLabSnapshot, String> {
    // 重复点击「开始录音」必须在动任何资源之前就挡住。
    //
    // 否则整条链路会一路走到底才失败：`acquire` 对同一 owner 会推进 generation 并顶掉
    // 旧租约，`start_backend_mic_inner` 对同一设备返回 reused，直到 `begin()` 才因为
    // 「正在录音」报错——而失败清理里的 `abort()` 是 `*state = default()`，正在进行的
    // 录音被整个清空、麦克风被停、租约被释放。用户只是手抖点了两下，录了一半的音频
    // 就没了，且不可恢复。
    let _operation = state.audio_lab_runtime.operation.lock().await;
    if state.audio_lab_runtime.is_recording()? {
        return Err("音频调校正在录音".into());
    }
    let lease = state
        .audio_session
        .acquire(crate::application::audio_session::AudioOwner::AudioLab)?;
    match state.audio_lab_lease.lock() {
        Ok(mut current) => *current = Some(lease),
        Err(_) => {
            let _ = state.audio_session.release(&lease);
            return Err("音频会话锁失败".into());
        }
    }
    let started = match crate::desktop::backend_mic::start_backend_mic_inner(device_name, &state) {
        Ok(started) => started,
        Err(error) => {
            // 还没 begin，上一次录好的素材不该被这次失败连累。
            cleanup_start_failure(&state, false);
            return Err(error);
        }
    };
    let epoch = match state.audio_lab_runtime.begin(started.sample_rate) {
        Ok(epoch) => epoch,
        Err(error) => {
            cleanup_start_failure(&state, false);
            return Err(error);
        }
    };
    let (_, mut receiver) = match crate::desktop::backend_mic::attach_backend_mic_raw_inner(
        &state,
        crate::state::AsrPreroll::Disabled,
    ) {
        Ok(attached) => attached,
        Err(error) => {
            cleanup_start_failure(&state, true);
            return Err(error);
        }
    };
    let (done, drain) = tokio::sync::oneshot::channel();
    if let Err(error) = state.audio_lab_runtime.register_drain(epoch, drain) {
        cleanup_start_failure(&state, true);
        return Err(error);
    }
    tauri::async_runtime::spawn_blocking(move || {
        let result = (|| {
            while let Some(samples) = receiver.blocking_recv()? {
                if done.is_closed() {
                    return Err("音频调校会话已结束".into());
                }
                let state = app
                    .try_state::<crate::state::RuntimeState>()
                    .ok_or("应用已退出")?;
                state.audio_lab_runtime.append_for(epoch, &samples)?;
            }
            let state = app
                .try_state::<crate::state::RuntimeState>()
                .ok_or("应用已退出")?;
            state.audio_lab_runtime.finish_input(epoch)
        })();
        // stop 持有 operation 等待排空，因此必须先交付结果再尝试取得清理锁。
        let _ = done.send(result.clone());
        if let Err(error) = result {
            let Some(state) = app.try_state::<crate::state::RuntimeState>() else {
                return;
            };
            let _operation =
                tauri::async_runtime::block_on(state.audio_lab_runtime.operation.lock());
            if state.audio_lab_runtime.fail_for(epoch, error) {
                let _ = crate::desktop::backend_mic::release_backend_mic_inner(&state);
                if let Err(error) = release_audio_lab_lease(&state) {
                    eprintln!("[audio-lab] 释放音频会话失败：{error}");
                }
                publish(&app);
            }
        }
    });
    state.audio_lab_runtime.snapshot()
}

/// `discard_session` 只有在本次确实已经 `begin()` 过时才该为真——`abort()` 是
/// `*state = default()`，对尚未开始的失败调用它会连上一次录好的素材一起抹掉。
fn cleanup_start_failure(state: &crate::state::RuntimeState, discard_session: bool) {
    if discard_session {
        state.audio_lab_runtime.abort();
    }
    let _ = crate::desktop::backend_mic::release_backend_mic_inner(state);
    if let Err(error) = release_audio_lab_lease(state) {
        eprintln!("[audio-lab] 释放音频会话失败：{error}");
    }
}

fn release_audio_lab_lease(state: &crate::state::RuntimeState) -> Result<(), String> {
    if let Some(lease) = state
        .audio_lab_lease
        .lock()
        .map_err(|_| "音频会话锁失败")?
        .take()
    {
        state.audio_session.release(&lease)?;
    }
    Ok(())
}

#[tauri::command]
pub(crate) async fn audio_lab_stop(
    state: tauri::State<'_, crate::state::RuntimeState>,
) -> Result<AudioLabSnapshot, String> {
    let _operation = state.audio_lab_runtime.operation.lock().await;
    if !state.audio_lab_runtime.is_recording()? {
        return state.audio_lab_runtime.snapshot();
    }
    // 先停止设备并关闭发送端，消费方排空尾包后才能把 recording 改为 false。
    let epoch = state.audio_lab_runtime.request_stop()?;
    let paused = crate::desktop::backend_mic::pause_backend_mic_inner(&state);
    let released = crate::desktop::backend_mic::release_backend_mic_inner(&state);
    let lease_released = release_audio_lab_lease(&state);
    let drained = state.audio_lab_runtime.drain_capture().await;
    if let Err(error) = paused.and(released).and(lease_released).and(drained) {
        state.audio_lab_runtime.fail_for(epoch, error.clone());
        return Err(error);
    }
    state.audio_lab_runtime.stop()?;
    state.audio_lab_runtime.snapshot()
}

#[tauri::command]
pub(crate) async fn audio_lab_reprocess(
    app: tauri::AppHandle,
    params: DspParams,
) -> Result<AudioLabSnapshot, String> {
    let runtime = &app.state::<crate::state::RuntimeState>().audio_lab_runtime;
    let revision = runtime.processing_revision.fetch_add(1, Ordering::AcqRel) + 1;
    let gate = runtime.processing_gate.clone().lock_owned().await;
    if runtime.processing_revision.load(Ordering::Acquire) != revision {
        return Err("音频处理已被新任务替换".into());
    }
    let target = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let _gate = gate;
        app.state::<crate::state::RuntimeState>()
            .audio_lab_runtime
            .reprocess_at(params, revision)
    })
    .await
    .map_err(|e| format!("音频处理任务失败：{e}"))?;
    super::idle_reclaim::request(&target);
    result
}

#[tauri::command]
pub(crate) async fn get_audio_lab_runtime(
    app: tauri::AppHandle,
) -> Result<AudioLabSnapshot, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<crate::state::RuntimeState>()
            .audio_lab_runtime
            .snapshot()
    })
    .await
    .map_err(|e| format!("读取音频调校状态失败：{e}"))?
}

#[tauri::command]
pub(crate) async fn audio_lab_audio_path(
    app: tauri::AppHandle,
    processed: bool,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<crate::state::RuntimeState>()
            .audio_lab_runtime
            .write_wav(processed)
    })
    .await
    .map_err(|error| format!("试听文件任务失败：{error}"))?
}

fn snapshot(state: &AudioLabState) -> Result<AudioLabSnapshot, String> {
    Ok(AudioLabSnapshot {
        recording: state.recording,
        sample_rate: state.sample_rate,
        duration_ms: state.raw.len() as u64 * 1000 / state.sample_rate.max(1) as u64,
        raw_waveform: state
            .raw
            .waveform(WAVE_POINTS)
            .map_err(|e| format!("读取原始波形失败：{e}"))?,
        processed_waveform: state
            .processed
            .waveform(WAVE_POINTS)
            .map_err(|e| format!("读取处理波形失败：{e}"))?,
        stats: state.stats.clone(),
        error: state.error.clone(),
    })
}
fn publish(app: &tauri::AppHandle) {
    let state = app.state::<crate::state::RuntimeState>();
    let revision = next_revision(&state.snapshot_revision);
    let payload = state
        .audio_lab_runtime
        .snapshot()
        .ok()
        .and_then(|snapshot| serde_json::to_value(snapshot).ok())
        .unwrap_or_else(|| json!({}));
    let _ = app.emit(
        "domain-event",
        DomainEventEnvelope {
            revision,
            domain: "audioLab".into(),
            event_type: "stateChanged".into(),
            session_id: None,
            payload,
        },
    );
}
#[cfg(test)]
fn summarize(samples: &[f32]) -> Vec<[f32; 2]> {
    if samples.is_empty() {
        return Vec::new();
    }
    let width = samples.len().min(WAVE_POINTS);
    (0..width)
        .map(|index| {
            let start = index * samples.len() / width;
            let end = ((index + 1) * samples.len() / width).max(start + 1);
            samples[start..end]
                .iter()
                .fold([1.0_f32, -1.0_f32], |[min, max], sample| {
                    [min.min(*sample), max.max(*sample)]
                })
        })
        .collect()
}

#[cfg(test)]
mod capture_tests;

#[cfg(test)]
mod tests {
    use super::*;

    /// 重复点击「开始录音」不得把正在进行的录音清空。
    ///
    /// 整条链路原本会一路走到 `begin()` 才失败：acquire 对同 owner 推进 generation、
    /// start_backend_mic_inner 对同设备返回 reused，而失败清理里的 `abort()` 是
    /// `*state = default()`——已经采到的音频、采样率、统计全被抹掉，麦克风被停、租约
    /// 被释放。用户只是手抖点了两下。
    #[test]
    fn restarting_while_recording_keeps_the_take_intact() {
        let runtime = AudioLabRuntime::default();
        runtime.begin(48_000).unwrap();
        runtime.append(&[0.1, -0.2, 0.3]);
        assert!(runtime.is_recording().unwrap());

        // 第二次 begin 必须被拒，且不得影响已有素材。
        assert!(runtime.begin(48_000).is_err());
        assert!(runtime.is_recording().unwrap(), "录音状态不能被顶掉");

        runtime.stop().expect("已有采样，停止应当成功");
        let snapshot = runtime.snapshot().unwrap();
        assert_eq!(snapshot.sample_rate, 48_000);
        assert!(snapshot.duration_ms > 0 || !snapshot.raw_waveform.is_empty());
    }

    /// 尚未 begin 的启动失败不该连累上一次录好的素材。
    #[test]
    fn a_failed_start_before_begin_keeps_previous_material() {
        let runtime = AudioLabRuntime::default();
        runtime.begin(16_000).unwrap();
        runtime.append(&[0.5; 256]);
        runtime.stop().unwrap();

        // 模拟 cleanup_start_failure(state, false)：不调用 abort。
        assert!(!runtime.is_recording().unwrap());
        let snapshot = runtime.snapshot().unwrap();
        assert_eq!(snapshot.sample_rate, 16_000, "上一次的素材必须还在");

        // 而 discard_session = true 才真正清场。
        runtime.abort();
        assert_eq!(runtime.snapshot().unwrap().sample_rate, 0);
    }

    /// 守卫必须在**申请任何资源之前**，否则整条链路会一路走到 begin() 才失败，
    /// 而那时失败清理已经把正在进行的录音连同麦克风和租约一起收掉了。
    #[test]
    fn start_command_rejects_a_second_press_before_touching_resources() {
        // 归一化行尾：按 core.autocrlf 检出时工作区是 CRLF，含 \n 的切片会失配。
        let source = include_str!("audio_lab.rs").replace("\r\n", "\n");
        let body = &source[..source
            .find("#[cfg(test)]\nmod tests")
            .expect("audio_lab.rs 必须有测试模块标记")];
        let start = body
            .find("pub(crate) async fn audio_lab_start")
            .expect("audio_lab_start 必须仍然存在");
        let command = &body[start..];
        let command = &command[..command.find("\n}\n").expect("函数体未闭合")];

        let guard_at = command
            .find("is_recording()")
            .expect("必须先判断是否正在录音");
        let acquire_at = command.find(".acquire(").expect("必须仍然申请音频租约");
        assert!(
            guard_at < acquire_at,
            "重复点击的守卫必须早于 acquire，否则旧租约会被顶掉"
        );
        assert!(
            command.contains("cleanup_start_failure(&state, false)"),
            "尚未 begin 的失败不得清空已有素材"
        );
    }

    #[test]
    fn waveform_is_bounded() {
        assert_eq!(summarize(&vec![0.0; 1000]).len(), WAVE_POINTS);
    }

    #[test]
    fn capture_failure_stops_recording_and_preserves_the_error() {
        let runtime = AudioLabRuntime::default();
        let epoch = runtime.begin(48_000).unwrap();
        runtime.append_for(epoch, &[0.1, -0.1]).unwrap();

        runtime.fail_for(epoch, "输入设备已断开".into());

        let snapshot = runtime.snapshot().unwrap();
        assert!(!snapshot.recording);
        assert_eq!(snapshot.error.as_deref(), Some("输入设备已断开"));
        assert_eq!(runtime.domain_snapshot().state, DomainRunState::Failed);
    }
}
