//! 音频调校会话：原始 PCM、离线 DSP 和波形摘要只驻留在 Rust。
use std::sync::Mutex;

use serde::Serialize;
use serde_json::json;
use tauri::{Emitter, Manager};

use crate::application::contract::{
    next_revision, DomainEventEnvelope, DomainRunState, DomainSnapshot,
};
use crate::audio_dsp::{process_offline, DspParams};

const WAVE_POINTS: usize = 860;

#[cfg(all(test, windows))]
mod performance_tests;

#[derive(Default)]
pub(crate) struct AudioLabRuntime {
    state: Mutex<AudioLabState>,
}

#[derive(Default)]
struct AudioLabState {
    recording: bool,
    sample_rate: u32,
    raw: Vec<f32>,
    processed: Vec<f32>,
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
    pub(crate) fn is_recording(&self) -> Result<bool, String> {
        self.state
            .lock()
            .map(|state| state.recording)
            .map_err(|_| "音频调校状态锁失败".into())
    }
    pub(crate) fn begin(&self, sample_rate: u32) -> Result<(), String> {
        let mut state = self.state.lock().map_err(|_| "音频调校状态锁失败")?;
        if state.recording {
            return Err("音频调校正在录音".into());
        }
        *state = AudioLabState {
            recording: true,
            sample_rate,
            ..Default::default()
        };
        Ok(())
    }
    pub(crate) fn append(&self, samples: &[f32]) {
        if let Ok(mut state) = self.state.lock() {
            if state.recording {
                state.raw.extend_from_slice(samples);
            }
        }
    }
    fn abort(&self) {
        if let Ok(mut state) = self.state.lock() {
            *state = AudioLabState::default();
        }
    }
    fn fail(&self, error: String) {
        if let Ok(mut state) = self.state.lock() {
            if state.recording {
                state.recording = false;
                state.error = Some(error);
            }
        }
    }
    pub(crate) fn stop(&self) -> Result<(), String> {
        let mut state = self.state.lock().map_err(|_| "音频调校状态锁失败")?;
        state.recording = false;
        if state.raw.is_empty() {
            return Err("未录到音频".into());
        }
        Ok(())
    }
    pub(crate) fn reprocess(&self, params: DspParams) -> Result<AudioLabSnapshot, String> {
        let mut state = self.state.lock().map_err(|_| "音频调校状态锁失败")?;
        if state.raw.is_empty() {
            return Err("请先录制音频".into());
        }
        let result = process_offline(&state.raw, state.sample_rate, &params);
        state.processed = result.processed;
        state.stats = Some(AudioLabStats {
            in_lufs: result.in_lufs,
            out_lufs: result.out_lufs,
            in_peak_db: result.in_peak_db,
            out_peak_db: result.out_peak_db,
            clipped_samples: state
                .processed
                .iter()
                .filter(|sample| sample.abs() >= 0.999)
                .count(),
        });
        Ok(snapshot(&state))
    }
    pub(crate) fn snapshot(&self) -> Result<AudioLabSnapshot, String> {
        let state = self.state.lock().map_err(|_| "音频调校状态锁失败")?;
        Ok(snapshot(&state))
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
        let state = self.state.lock().map_err(|_| "音频调校状态锁失败")?;
        let samples = if processed {
            &state.processed
        } else {
            &state.raw
        };
        if samples.is_empty() {
            return Err("没有可播放的音频".into());
        }
        let rate = if processed { 48_000 } else { state.sample_rate };
        let data_len = (samples.len() * 2) as u32;
        let mut bytes = Vec::with_capacity(44 + data_len as usize);
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&rate.to_le_bytes());
        bytes.extend_from_slice(&(rate * 2).to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data_len.to_le_bytes());
        for sample in samples {
            bytes.extend_from_slice(
                &((sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16).to_le_bytes(),
            );
        }
        let path = std::env::temp_dir().join(format!(
            "say-it-audio-lab-{}.wav",
            if processed { "processed" } else { "raw" }
        ));
        std::fs::write(&path, bytes).map_err(|error| format!("写入试听文件失败：{error}"))?;
        path.to_str()
            .map(str::to_owned)
            .ok_or_else(|| "试听文件路径无效".into())
    }
}

#[tauri::command]
pub(crate) fn audio_lab_start(
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
    if let Err(error) = state.audio_lab_runtime.begin(started.sample_rate) {
        cleanup_start_failure(&state, false);
        return Err(error);
    }
    let (_, mut receiver) = match crate::desktop::backend_mic::attach_backend_mic_raw_inner(&state)
    {
        Ok(attached) => attached,
        Err(error) => {
            cleanup_start_failure(&state, true);
            return Err(error);
        }
    };
    tauri::async_runtime::spawn(async move {
        while let Some(crate::state::AsrStreamInput::RawF32(samples)) = receiver.recv().await {
            if let Some(runtime) = app.try_state::<crate::state::RuntimeState>() {
                runtime.audio_lab_runtime.append(&samples);
            }
        }
        let Some(state) = app.try_state::<crate::state::RuntimeState>() else {
            return;
        };
        let capture_error = state
            .backend_mic
            .lock()
            .ok()
            .and_then(|mut capture| capture.last_error.take());
        if let Some(error) = capture_error {
            state.audio_lab_runtime.fail(error);
            let _ = crate::desktop::backend_mic::release_backend_mic_inner(&state);
            if let Ok(mut current) = state.audio_lab_lease.lock() {
                if let Some(lease) = current.take() {
                    let _ = state.audio_session.release(&lease);
                }
            }
            publish(&app);
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
    if let Ok(mut current) = state.audio_lab_lease.lock() {
        if let Some(lease) = current.take() {
            let _ = state.audio_session.release(&lease);
        }
    }
}

#[tauri::command]
pub(crate) fn audio_lab_stop(
    state: tauri::State<'_, crate::state::RuntimeState>,
) -> Result<AudioLabSnapshot, String> {
    crate::desktop::backend_mic::pause_backend_mic_inner(&state)?;
    crate::desktop::backend_mic::release_backend_mic_inner(&state)?;
    if let Some(lease) = state
        .audio_lab_lease
        .lock()
        .map_err(|_| "音频会话锁失败")?
        .take()
    {
        state.audio_session.release(&lease)?;
    }
    state.audio_lab_runtime.stop()?;
    state.audio_lab_runtime.snapshot()
}

#[tauri::command]
pub(crate) fn audio_lab_reprocess(
    state: tauri::State<'_, crate::state::RuntimeState>,
    params: DspParams,
) -> Result<AudioLabSnapshot, String> {
    state.audio_lab_runtime.reprocess(params)
}

#[tauri::command]
pub(crate) fn get_audio_lab_runtime(
    state: tauri::State<'_, crate::state::RuntimeState>,
) -> Result<AudioLabSnapshot, String> {
    state.audio_lab_runtime.snapshot()
}

#[tauri::command]
pub(crate) fn audio_lab_audio_path(
    state: tauri::State<'_, crate::state::RuntimeState>,
    processed: bool,
) -> Result<String, String> {
    state.audio_lab_runtime.write_wav(processed)
}

fn snapshot(state: &AudioLabState) -> AudioLabSnapshot {
    AudioLabSnapshot {
        recording: state.recording,
        sample_rate: state.sample_rate,
        duration_ms: state.raw.len() as u64 * 1000 / state.sample_rate.max(1) as u64,
        raw_waveform: summarize(&state.raw),
        processed_waveform: summarize(&state.processed),
        stats: state.stats.clone(),
        error: state.error.clone(),
    }
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
            .find("#[cfg(test)]")
            .expect("audio_lab.rs 必须有测试模块标记")];
        let start = body
            .find("pub(crate) fn audio_lab_start")
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
        runtime.begin(48_000).unwrap();
        runtime.append(&[0.1, -0.1]);

        runtime.fail("输入设备已断开".into());

        let snapshot = runtime.snapshot().unwrap();
        assert!(!snapshot.recording);
        assert_eq!(snapshot.error.as_deref(), Some("输入设备已断开"));
        assert_eq!(runtime.domain_snapshot().state, DomainRunState::Failed);
    }
}
