//! 音频处理：神经网络降噪（nnnoiseless / RNNoise）+ EBU R128 响度归一化（ebur128）。
//!
//! 两条使用路径共用同一套参数 [`DspParams`]：
//! - 离线：调校台录一段 → [`process_offline`] 返回处理后的 48k PCM 与前后 LUFS/峰值，供 A/B 试听。
//! - 实时：语音输入把麦克风原始音频喂给 [`StreamDsp::process`]，得到可直接送 ASR 的 16k PCM16。
//!
//! RNNoise 固定工作在 48kHz、480 样本/帧，且样本范围是 i16（±32768），而 ebur128 用 [-1,1]。
//! 本模块内部统一以 [-1,1] f32 表示，在喂给 RNNoise 前后做缩放。

use ebur128::{EbuR128, Mode};
use nnnoiseless::DenoiseState;
use serde::{Deserialize, Serialize};

const FRAME: usize = 480; // 48kHz 下 10ms
const RATE_48K: u32 = 48_000;
const RATE_16K: u32 = 16_000;
/// 响度极低（接近数字静音）时不再据此调增益，避免把纯底噪顶上来。
/// 这里不能太高：远麦克风的弱语音可能低于 -50 LUFS，实时链路仍应把它拉起来。
const SILENCE_LUFS: f32 = -90.0;

fn d_true() -> bool {
    true
}
fn d_strength() -> f32 {
    1.0
}
fn d_target() -> f32 {
    -20.0
}
fn d_peak() -> f32 {
    -1.0
}
fn d_maxgain() -> f32 {
    40.0
}
fn d_vad() -> f32 {
    0.0
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DspParams {
    /// 是否启用 RNNoise 降噪。
    #[serde(default = "d_true")]
    pub denoise_enabled: bool,
    /// 降噪强度（干/湿混合，0=不降噪，1=全降噪）。
    #[serde(default = "d_strength")]
    pub denoise_strength: f32,
    /// 目标整体响度（LUFS），归一化把语音拉到这个水平。
    #[serde(default = "d_target")]
    pub target_lufs: f32,
    /// 输出峰值上限（dBFS），硬限幅避免削波。
    #[serde(default = "d_peak")]
    pub peak_limit_dbfs: f32,
    /// 最大允许提升的增益（dB），防止把近似静音的底噪放大过头。
    #[serde(default = "d_maxgain")]
    pub max_gain_db: f32,
    /// 语音门：RNNoise 给出的 VAD 概率低于此值的帧直接静音（0=关闭，需开降噪才生效）。
    #[serde(default = "d_vad")]
    pub vad_gate: f32,
}

impl Default for DspParams {
    fn default() -> Self {
        Self {
            denoise_enabled: d_true(),
            denoise_strength: d_strength(),
            target_lufs: d_target(),
            peak_limit_dbfs: d_peak(),
            max_gain_db: d_maxgain(),
            vad_gate: d_vad(),
        }
    }
}

fn db_to_lin(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

fn lin_to_db(x: f32) -> f32 {
    if x <= 1e-9 {
        f32::NEG_INFINITY
    } else {
        20.0 * x.log10()
    }
}

fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum = samples.iter().fold(0.0, |acc, &x| acc + x * x);
    (sum / samples.len() as f32).sqrt()
}

/// 简单线性重采样到 48kHz（仅在麦克风非 48k 时用；离线一次性版本）。
fn resample_to_48k(input: &[f32], in_rate: u32) -> Vec<f32> {
    if in_rate == RATE_48K || input.is_empty() {
        return input.to_vec();
    }
    let ratio = RATE_48K as f64 / in_rate as f64;
    let out_len = ((input.len() as f64) * ratio).round() as usize;
    let mut out = Vec::with_capacity(out_len);
    for k in 0..out_len {
        let pos = k as f64 / ratio;
        let i = pos.floor() as usize;
        let frac = (pos - i as f64) as f32;
        let a = input[i.min(input.len() - 1)];
        let b = input[(i + 1).min(input.len() - 1)];
        out.push(a + (b - a) * frac);
    }
    out
}

fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0f32, |m, &x| m.max(x.abs()))
}

/// 用 ebur128 测整段积分响度（LUFS）。样本不足或无声时返回 NEG_INFINITY。
fn integrated_lufs(samples: &[f32]) -> f32 {
    let mut m = match EbuR128::new(1, RATE_48K, Mode::I) {
        Ok(m) => m,
        Err(_) => return f32::NEG_INFINITY,
    };
    if m.add_frames_f32(samples).is_err() {
        return f32::NEG_INFINITY;
    }
    match m.loudness_global() {
        Ok(v) if v.is_finite() => v as f32,
        _ => f32::NEG_INFINITY,
    }
}

/// 对整段 48k 信号做 RNNoise 降噪（含干/湿混合与 VAD 门）。返回 [-1,1] f32。
fn denoise_all(s48: &[f32], strength: f32, vad_gate: f32) -> Vec<f32> {
    let mut st = DenoiseState::new();
    let mut out = vec![0f32; s48.len()];
    let mut inf = [0f32; FRAME];
    let mut outf = [0f32; FRAME];
    let mut i = 0;
    while i < s48.len() {
        let n = (s48.len() - i).min(FRAME);
        for j in 0..FRAME {
            inf[j] = if j < n { s48[i + j] * 32768.0 } else { 0.0 };
        }
        let vad = st.process_frame(&mut outf, &inf);
        let g = if vad_gate > 0.0 && vad < vad_gate {
            0.0
        } else {
            1.0
        };
        for j in 0..n {
            let wet = outf[j] / 32768.0;
            let dry = s48[i + j];
            out[i + j] = (dry * (1.0 - strength) + wet * strength) * g;
        }
        i += n;
    }
    out
}

/// 离线处理结果（供调校台 A/B 试听与读数）。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OfflineResult {
    /// 处理后 PCM（f32 小端字节，base64）。
    pub processed_base64: String,
    pub sample_rate: u32,
    pub in_lufs: f32,
    pub out_lufs: f32,
    pub in_peak_db: f32,
    pub out_peak_db: f32,
}

fn f32_to_base64(samples: &[f32]) -> String {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    let mut bytes = Vec::with_capacity(samples.len() * 4);
    for &s in samples {
        bytes.extend_from_slice(&s.to_le_bytes());
    }
    STANDARD.encode(bytes)
}

fn nan_to_neg(x: f32) -> f32 {
    if x.is_finite() {
        x
    } else {
        -120.0
    }
}

/// 离线处理一整段录音：降噪 → 响度归一化到目标 LUFS → 峰值限幅。
pub fn process_offline(input: &[f32], in_rate: u32, params: &DspParams) -> OfflineResult {
    let s48 = resample_to_48k(input, in_rate);
    let in_lufs = integrated_lufs(&s48);
    let in_peak_db = lin_to_db(peak(&s48));

    let wet = if params.denoise_enabled {
        denoise_all(&s48, params.denoise_strength, params.vad_gate)
    } else {
        s48
    };

    // 据降噪后的响度算需要的增益，限制最大提升量。
    let wet_lufs = integrated_lufs(&wet);
    let mut gain_db = if wet_lufs.is_finite() {
        params.target_lufs - wet_lufs
    } else {
        0.0
    };
    if gain_db > params.max_gain_db {
        gain_db = params.max_gain_db;
    }
    let gain = db_to_lin(gain_db);
    let peak_lin = db_to_lin(params.peak_limit_dbfs);

    let out: Vec<f32> = wet
        .iter()
        .map(|&x| (x * gain).clamp(-peak_lin, peak_lin))
        .collect();

    let out_lufs = integrated_lufs(&out);
    let out_peak_db = lin_to_db(peak(&out));

    OfflineResult {
        processed_base64: f32_to_base64(&out),
        sample_rate: RATE_48K,
        in_lufs: nan_to_neg(in_lufs),
        out_lufs: nan_to_neg(out_lufs),
        in_peak_db: nan_to_neg(in_peak_db),
        out_peak_db: nan_to_neg(out_peak_db),
    }
}

/// 实时流式处理器：把麦克风原始音频转成可直接送 ASR 的 16k PCM16。
/// 内部维持降噪状态、动量响度计与平滑增益，跨多次 `process` 连续。
pub struct StreamDsp {
    params: DspParams,
    denoise: Box<DenoiseState<'static>>,
    meter: EbuR128,
    gain: f32,
    in_rate: u32,
    // 线性重采样到 48k 的连续状态（仅非 48k 时使用）。
    rs_step: f64,
    rs_t: f64,
    rs_prev: f32,
    rs_init: bool,
    rs_in_idx: u64,
    buf48: Vec<f32>,
    // 48k→16k 三抽一盒式平均状态。
    dec_acc: f32,
    dec_cnt: u32,
    peak_lin: f32,
}

impl StreamDsp {
    pub fn new(params: DspParams, in_rate: u32) -> Self {
        let meter = EbuR128::new(1, RATE_48K, Mode::M).expect("ebur128 init");
        let peak_lin = db_to_lin(params.peak_limit_dbfs);
        let in_rate = if in_rate == 0 { RATE_48K } else { in_rate };
        Self {
            params,
            denoise: DenoiseState::new(),
            meter,
            gain: 1.0,
            in_rate,
            rs_step: in_rate as f64 / RATE_48K as f64,
            rs_t: 0.0,
            rs_prev: 0.0,
            rs_init: false,
            rs_in_idx: 0,
            buf48: Vec::new(),
            dec_acc: 0.0,
            dec_cnt: 0,
            peak_lin,
        }
    }

    fn resample_into(&mut self, input: &[f32], out: &mut Vec<f32>) {
        if self.in_rate == RATE_48K {
            out.extend_from_slice(input);
            return;
        }
        for &x in input {
            if !self.rs_init {
                self.rs_init = true;
                self.rs_prev = x;
                self.rs_in_idx = 1;
                self.rs_t = self.rs_step;
                out.push(x); // 第一帧对齐到输入起点
                continue;
            }
            let cur = self.rs_in_idx as f64;
            while self.rs_t <= cur {
                let frac = (self.rs_t - (cur - 1.0)) as f32;
                out.push(self.rs_prev + (x - self.rs_prev) * frac);
                self.rs_t += self.rs_step;
            }
            self.rs_prev = x;
            self.rs_in_idx += 1;
        }
    }

    /// 输入麦克风原始 f32（in_rate，[-1,1]），输出 16k PCM16 小端字节（可能为空，凑够一帧才出）。
    pub fn process(&mut self, input: &[f32]) -> Vec<u8> {
        let mut resampled = Vec::new();
        self.resample_into(input, &mut resampled);
        self.buf48.append(&mut resampled);

        let mut out16: Vec<f32> = Vec::new();
        let mut inf = [0f32; FRAME];
        let mut outf = [0f32; FRAME];
        let mut wet = [0f32; FRAME];

        while self.buf48.len() >= FRAME {
            let strength = self.params.denoise_strength;
            let mut vadg = 1.0f32;
            if self.params.denoise_enabled {
                for j in 0..FRAME {
                    inf[j] = self.buf48[j] * 32768.0;
                }
                let vad = self.denoise.process_frame(&mut outf, &inf);
                if self.params.vad_gate > 0.0 && vad < self.params.vad_gate {
                    vadg = 0.0;
                }
                for j in 0..FRAME {
                    let w = outf[j] / 32768.0;
                    let dry = self.buf48[j];
                    wet[j] = (dry * (1.0 - strength) + w * strength) * vadg;
                }
            } else {
                wet[..FRAME].copy_from_slice(&self.buf48[..FRAME]);
            }

            // 用降噪后的动量响度驱动自适应增益。
            // ebur128 的 momentary loudness 需要一小段历史；如果暂时拿不到，使用当前
            // RNNoise 帧的 RMS 作为保守 fallback，避免远麦克风开头一直不被增益拉起。
            let _ = self.meter.add_frames_f32(&wet);
            let meter_lufs = self
                .meter
                .loudness_momentary()
                .ok()
                .map(|v| v as f32)
                .filter(|v| v.is_finite())
                .unwrap_or(f32::NEG_INFINITY);
            let frame_lufs = lin_to_db(rms(&wet));
            let m = if meter_lufs > SILENCE_LUFS {
                meter_lufs
            } else {
                frame_lufs
            };

            let desired = if m > SILENCE_LUFS {
                let mut gdb = self.params.target_lufs - m;
                if gdb > self.params.max_gain_db {
                    gdb = self.params.max_gain_db;
                }
                if gdb < -12.0 {
                    gdb = -12.0;
                }
                db_to_lin(gdb)
            } else {
                self.gain
            };
            // 提升慢一点、回落快一点，避免抽气感与削波。
            let coeff = if desired > self.gain { 0.08 } else { 0.03 };
            self.gain += (desired - self.gain) * coeff;

            for j in 0..FRAME {
                let mut s = wet[j] * self.gain;
                if s > self.peak_lin {
                    s = self.peak_lin;
                } else if s < -self.peak_lin {
                    s = -self.peak_lin;
                }
                self.dec_acc += s;
                self.dec_cnt += 1;
                if self.dec_cnt == 3 {
                    out16.push(self.dec_acc / 3.0);
                    self.dec_acc = 0.0;
                    self.dec_cnt = 0;
                }
            }

            self.buf48.drain(0..FRAME);
        }

        let mut bytes = Vec::with_capacity(out16.len() * 2);
        for &s in &out16 {
            let c = s.clamp(-1.0, 1.0);
            let v = (if c < 0.0 { c * 32768.0 } else { c * 32767.0 }) as i16;
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        bytes
    }
}

// 16k 是输出采样率常量，导出供主模块在日志里引用（统计时长）。
pub const OUTPUT_RATE: u32 = RATE_16K;
