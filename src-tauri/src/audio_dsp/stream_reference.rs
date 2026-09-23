//! 优化前的实时 DSP 缓冲算法，冻结为测试参考；不得跟随生产缓冲重构同步修改。
use super::*;

pub(super) struct ReferenceStreamDsp {
    params: DspParams,
    denoise: Box<DenoiseState<'static>>,
    eq: ShelfEq,
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

impl ReferenceStreamDsp {
    pub fn new(params: DspParams, in_rate: u32) -> Self {
        let meter = EbuR128::new(1, RATE_48K, Mode::M).expect("ebur128 init");
        let peak_lin = db_to_lin(params.peak_limit_dbfs);
        let in_rate = if in_rate == 0 { RATE_48K } else { in_rate };
        let eq = ShelfEq::new(RATE_48K as f32, params.bass_gain_db, params.treble_gain_db);
        Self {
            params,
            denoise: DenoiseState::new(),
            eq,
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
            self.eq.process_slice(&mut wet);

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
            let coeff = gain_smoothing_coeff(desired, self.gain);
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
