//! 离线 DSP 两遍分块处理；全段响度和增益计算顺序与原实现一致。
use super::*;
use crate::audio_storage::{AudioBuffer, BLOCK};

pub(crate) struct StoredResult {
    pub(crate) processed: AudioBuffer,
    pub(crate) in_lufs: f32,
    pub(crate) out_lufs: f32,
    pub(crate) in_peak_db: f32,
    pub(crate) out_peak_db: f32,
    pub(crate) clipped_samples: usize,
}

fn meter() -> Result<EbuR128, String> {
    EbuR128::new(1, RATE_48K, Mode::I).map_err(|error| format!("创建响度计失败：{error}"))
}
fn measure(meter: &mut EbuR128, samples: &[f32]) -> Result<(), String> {
    meter
        .add_frames_f32(samples)
        .map_err(|error| format!("测量响度失败：{error}"))
}
fn loudness(meter: &EbuR128) -> f32 {
    match meter.loudness_global() {
        Ok(value) if value.is_finite() => value as f32,
        _ => f32::NEG_INFINITY,
    }
}

// 绝对输出位置保持旧离线插值的浮点运算顺序；不能换成累加相位的实时重采样器。
struct Input<'a> {
    audio: &'a AudioBuffer,
    start: usize,
    count: usize,
    cache: [f32; BLOCK],
}
impl Input<'_> {
    fn at(&mut self, position: usize) -> Result<f32, String> {
        let position = position.min(self.audio.len() - 1);
        if position < self.start || position >= self.start + self.count {
            self.start = position / BLOCK * BLOCK;
            self.count = (self.audio.len() - self.start).min(BLOCK);
            self.audio
                .read(self.start, &mut self.cache[..self.count])
                .map_err(|e| format!("读取原始音频失败：{e}"))?;
        }
        Ok(self.cache[position - self.start])
    }
}

pub(crate) fn process_cancellable(
    input: &AudioBuffer,
    in_rate: u32,
    params: &DspParams,
    cancelled: impl Fn() -> bool,
) -> Result<StoredResult, String> {
    if in_rate == 0 {
        return Err("音频采样率无效".into());
    }
    let ratio = RATE_48K as f64 / in_rate as f64;
    let length = (input.len() as f64 * ratio).round() as usize;
    let mut source = Input {
        audio: input,
        start: 0,
        count: 0,
        cache: [0.0; BLOCK],
    };
    let mut wet = AudioBuffer::default();
    let mut input_meter = meter()?;
    let mut wet_meter = meter()?;
    let mut input_peak = 0f32;
    let mut denoise = params.denoise_enabled.then(DenoiseState::new);
    let mut eq = ShelfEq::new(RATE_48K as f32, params.bass_gain_db, params.treble_gain_db);
    let mut chunk = [0f32; FRAME * 32];
    let mut noise_in = [0f32; FRAME];
    let mut noise_out = [0f32; FRAME];
    for start in (0..length).step_by(chunk.len()) {
        if cancelled() {
            return Err("音频处理已被新任务替换".into());
        }
        let n = (length - start).min(chunk.len());
        let samples = &mut chunk[..n];
        if in_rate == RATE_48K {
            input
                .read(start, samples)
                .map_err(|e| format!("读取原始音频失败：{e}"))?;
        } else {
            for (index, sample) in samples.iter_mut().enumerate() {
                let position = (start + index) as f64 / ratio;
                let i = position.floor() as usize;
                let fraction = (position - i as f64) as f32;
                let a = source.at(i)?;
                let b = source.at(i + 1)?;
                *sample = a + (b - a) * fraction;
            }
        }
        measure(&mut input_meter, samples)?;
        input_peak = input_peak.max(peak(samples));
        if let Some(state) = &mut denoise {
            for frame in samples.chunks_mut(FRAME) {
                for j in 0..FRAME {
                    noise_in[j] = if j < frame.len() {
                        frame[j] * 32768.0
                    } else {
                        0.0
                    };
                }
                let vad = state.process_frame(&mut noise_out, &noise_in);
                let gate = if params.vad_gate > 0.0 && vad < params.vad_gate {
                    0.0
                } else {
                    1.0
                };
                for (j, sample) in frame.iter_mut().enumerate() {
                    let processed = noise_out[j] / 32768.0;
                    *sample = (*sample * (1.0 - params.denoise_strength)
                        + processed * params.denoise_strength)
                        * gate;
                }
            }
        }
        eq.process_slice(samples);
        measure(&mut wet_meter, samples)?;
        wet.append(samples)
            .map_err(|e| format!("暂存处理音频失败：{e}"))?;
    }
    let in_lufs = loudness(&input_meter);
    let wet_lufs = loudness(&wet_meter);
    // 释放两份响度历史后再创建输出响度计，避免三份历史同时驻留。
    drop(input_meter);
    drop(wet_meter);
    let mut gain_db = if wet_lufs.is_finite() {
        params.target_lufs - wet_lufs
    } else {
        0.0
    };
    if gain_db > params.max_gain_db {
        gain_db = params.max_gain_db;
    }
    let gain = db_to_lin(gain_db);
    let limit = db_to_lin(params.peak_limit_dbfs);
    let mut output_meter = meter()?;
    let mut output_peak = 0f32;
    let mut clipped_samples = 0;
    for start in (0..length).step_by(chunk.len()) {
        if cancelled() {
            return Err("音频处理已被新任务替换".into());
        }
        let n = (length - start).min(chunk.len());
        let samples = &mut chunk[..n];
        wet.read(start, samples)
            .map_err(|e| format!("读取处理音频失败：{e}"))?;
        for sample in samples.iter_mut() {
            *sample = (*sample * gain).clamp(-limit, limit);
            clipped_samples += usize::from(sample.abs() >= 0.999);
        }
        measure(&mut output_meter, samples)?;
        output_peak = output_peak.max(peak(samples));
        wet.replace(start, samples)
            .map_err(|e| format!("写入处理音频失败：{e}"))?;
    }
    Ok(StoredResult {
        processed: wet,
        in_lufs: nan_to_neg(in_lufs),
        out_lufs: nan_to_neg(loudness(&output_meter)),
        in_peak_db: nan_to_neg(lin_to_db(input_peak)),
        out_peak_db: nan_to_neg(lin_to_db(output_peak)),
        clipped_samples,
    })
}

#[cfg(test)]
mod tests;

#[cfg(test)]
fn process(input: &AudioBuffer, rate: u32, params: &DspParams) -> Result<StoredResult, String> {
    process_cancellable(input, rate, params, || false)
}
