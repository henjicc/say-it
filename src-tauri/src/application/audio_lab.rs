//! 音频调校会话：原始 PCM、离线 DSP 和波形摘要只驻留在 Rust。
use std::sync::Mutex;

use serde::Serialize;
use tauri::Manager;

use crate::audio_dsp::{process_offline, DspParams};
use crate::application::contract::{DomainRunState, DomainSnapshot};

const WAVE_POINTS: usize = 860;

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
}

impl AudioLabRuntime {
    pub(crate) fn begin(&self, sample_rate: u32) -> Result<(), String> {
        let mut state = self.state.lock().map_err(|_| "音频调校状态锁失败")?;
        if state.recording { return Err("音频调校正在录音".into()); }
        *state = AudioLabState { recording: true, sample_rate, ..Default::default() };
        Ok(())
    }
    pub(crate) fn append(&self, samples: &[f32]) {
        if let Ok(mut state) = self.state.lock() { if state.recording { state.raw.extend_from_slice(samples); } }
    }
    pub(crate) fn stop(&self) -> Result<(), String> {
        let mut state = self.state.lock().map_err(|_| "音频调校状态锁失败")?;
        state.recording = false;
        if state.raw.is_empty() { return Err("未录到音频".into()); }
        Ok(())
    }
    pub(crate) fn reprocess(&self, params: DspParams) -> Result<AudioLabSnapshot, String> {
        let mut state = self.state.lock().map_err(|_| "音频调校状态锁失败")?;
        if state.raw.is_empty() { return Err("请先录制音频".into()); }
        let result = process_offline(&state.raw, state.sample_rate, &params);
        state.processed = crate::state::decode_f32_base64(&result.processed_base64)?;
        state.stats = Some(AudioLabStats { in_lufs: result.in_lufs, out_lufs: result.out_lufs, in_peak_db: result.in_peak_db, out_peak_db: result.out_peak_db, clipped_samples: state.processed.iter().filter(|sample| sample.abs() >= 0.999).count() });
        Ok(snapshot(&state))
    }
    pub(crate) fn snapshot(&self) -> Result<AudioLabSnapshot, String> {
        let state = self.state.lock().map_err(|_| "音频调校状态锁失败")?;
        Ok(snapshot(&state))
    }
    pub(crate) fn domain_snapshot(&self) -> DomainSnapshot {
        match self.state.lock() { Ok(state) => DomainSnapshot { state: if state.recording { DomainRunState::Running } else { DomainRunState::Idle }, session_id: None }, Err(_) => DomainSnapshot { state: DomainRunState::Failed, session_id: None } }
    }
    pub(crate) fn write_wav(&self, processed: bool) -> Result<String, String> {
        let state = self.state.lock().map_err(|_| "音频调校状态锁失败")?;
        let samples = if processed { &state.processed } else { &state.raw };
        if samples.is_empty() { return Err("没有可播放的音频".into()); }
        let rate = if processed { 48_000 } else { state.sample_rate };
        let data_len = (samples.len() * 2) as u32;
        let mut bytes = Vec::with_capacity(44 + data_len as usize);
        bytes.extend_from_slice(b"RIFF"); bytes.extend_from_slice(&(36 + data_len).to_le_bytes()); bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes()); bytes.extend_from_slice(&1u16.to_le_bytes()); bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&rate.to_le_bytes()); bytes.extend_from_slice(&(rate * 2).to_le_bytes()); bytes.extend_from_slice(&2u16.to_le_bytes()); bytes.extend_from_slice(&16u16.to_le_bytes()); bytes.extend_from_slice(b"data"); bytes.extend_from_slice(&data_len.to_le_bytes());
        for sample in samples { bytes.extend_from_slice(&((sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16).to_le_bytes()); }
        let path = std::env::temp_dir().join(format!("say-it-audio-lab-{}.wav", if processed { "processed" } else { "raw" }));
        std::fs::write(&path, bytes).map_err(|error| format!("写入试听文件失败：{error}"))?;
        path.to_str().map(str::to_owned).ok_or_else(|| "试听文件路径无效".into())
    }
}

#[tauri::command]
pub(crate) fn audio_lab_start(app: tauri::AppHandle, state: tauri::State<'_, crate::state::RuntimeState>, device_name: Option<String>) -> Result<AudioLabSnapshot, String> {
    let lease = state.audio_session.acquire(crate::application::audio_session::AudioOwner::AudioLab)?;
    *state.audio_lab_lease.lock().map_err(|_| "音频会话锁失败")? = Some(lease);
    let started = crate::desktop::backend_mic::start_backend_mic_inner(device_name, &state)?;
    state.audio_lab_runtime.begin(started.sample_rate)?;
    let (_, mut receiver) = crate::desktop::backend_mic::attach_backend_mic_raw_inner(&state)?;
    tauri::async_runtime::spawn(async move {
        while let Some(crate::state::AsrStreamInput::RawF32(samples)) = receiver.recv().await {
            if let Some(runtime) = app.try_state::<crate::state::RuntimeState>() { runtime.audio_lab_runtime.append(&samples); }
        }
    });
    state.audio_lab_runtime.snapshot()
}

#[tauri::command]
pub(crate) fn audio_lab_stop(state: tauri::State<'_, crate::state::RuntimeState>) -> Result<AudioLabSnapshot, String> {
    crate::desktop::backend_mic::pause_backend_mic_inner(&state)?;
    crate::desktop::backend_mic::release_backend_mic_inner(&state)?;
    if let Some(lease) = state.audio_lab_lease.lock().map_err(|_| "音频会话锁失败")?.take() { state.audio_session.release(&lease)?; }
    state.audio_lab_runtime.stop()?;
    state.audio_lab_runtime.snapshot()
}

#[tauri::command]
pub(crate) fn audio_lab_reprocess(state: tauri::State<'_, crate::state::RuntimeState>, params: DspParams) -> Result<AudioLabSnapshot, String> { state.audio_lab_runtime.reprocess(params) }

#[tauri::command]
pub(crate) fn get_audio_lab_runtime(state: tauri::State<'_, crate::state::RuntimeState>) -> Result<AudioLabSnapshot, String> { state.audio_lab_runtime.snapshot() }

#[tauri::command]
pub(crate) fn audio_lab_audio_path(state: tauri::State<'_, crate::state::RuntimeState>, processed: bool) -> Result<String, String> { state.audio_lab_runtime.write_wav(processed) }

fn snapshot(state: &AudioLabState) -> AudioLabSnapshot {
    AudioLabSnapshot { recording: state.recording, sample_rate: state.sample_rate, duration_ms: state.raw.len() as u64 * 1000 / state.sample_rate.max(1) as u64, raw_waveform: summarize(&state.raw), processed_waveform: summarize(&state.processed), stats: state.stats.clone() }
}
fn summarize(samples: &[f32]) -> Vec<[f32; 2]> {
    if samples.is_empty() { return Vec::new(); }
    let width = samples.len().min(WAVE_POINTS);
    (0..width).map(|index| { let start = index * samples.len() / width; let end = ((index + 1) * samples.len() / width).max(start + 1); samples[start..end].iter().fold([1.0_f32, -1.0_f32], |[min, max], sample| [min.min(*sample), max.max(*sample)]) }).collect()
}

#[cfg(test)] mod tests { use super::*; #[test] fn waveform_is_bounded() { assert_eq!(summarize(&vec![0.0; 1000]).len(), WAVE_POINTS); } }
