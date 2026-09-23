//! 文件解码的连续线性重采样。保留全局采样位置，不能逐包调用离线重采样后拼接。
use super::TARGET_SAMPLE_RATE;

const OUTPUT_CHUNK: usize = 4096;

pub(super) struct MonoResampler {
    ratio: f64,
    input_count: usize,
    output_count: usize,
    pending_start: usize,
    pending: Vec<f32>,
    output: Vec<f32>,
}

impl MonoResampler {
    pub fn new(input_rate: u32) -> Result<Self, String> {
        if input_rate == 0 {
            return Err("音频采样率不能为零".into());
        }
        Ok(Self {
            ratio: TARGET_SAMPLE_RATE as f64 / input_rate as f64,
            input_count: 0,
            output_count: 0,
            pending_start: 0,
            pending: Vec::new(),
            output: Vec::with_capacity(OUTPUT_CHUNK),
        })
    }

    pub fn push(
        &mut self,
        samples: &[f32],
        consume: &mut impl FnMut(&[f32]) -> Result<(), String>,
    ) -> Result<(), String> {
        if samples.is_empty() {
            return Ok(());
        }
        self.input_count = self
            .input_count
            .checked_add(samples.len())
            .ok_or("音频过长")?;
        if self.ratio == 1.0 {
            // 同采样率时不做浮点运算，保留原始样本位模式。
            for chunk in samples.chunks(OUTPUT_CHUNK) {
                consume(chunk)?;
                self.output_count += chunk.len();
            }
            return Ok(());
        }
        self.pending.extend_from_slice(samples);
        self.emit(false, consume)?;

        // 仅保留下个插值位置及其后的样本。高倍率降采样也要等累计输出长度
        // 四舍五入后确认该样本存在，因此不能只保留上一包的最后一个样本。
        let next_source = (self.output_count as f64 / self.ratio).floor() as usize;
        let retain_from = next_source.min(self.input_count - 1);
        self.pending.drain(..retain_from - self.pending_start);
        self.pending_start = retain_from;
        Ok(())
    }

    pub fn finish(
        mut self,
        consume: &mut impl FnMut(&[f32]) -> Result<(), String>,
    ) -> Result<u64, String> {
        if self.ratio != 1.0 && self.input_count > 0 {
            self.emit(true, consume)?;
        }
        Ok(self.output_count as u64)
    }

    fn emit(
        &mut self,
        finished: bool,
        consume: &mut impl FnMut(&[f32]) -> Result<(), String>,
    ) -> Result<(), String> {
        let length = (self.input_count as f64 * self.ratio).round() as usize;
        while self.output_count < length {
            // 运算顺序与 audio_dsp::resample_linear 一致，避免分包后累积相位误差。
            let position = self.output_count as f64 / self.ratio;
            let index = position.floor() as usize;
            if !finished && index + 1 >= self.input_count {
                break;
            }
            let fraction = (position - index as f64) as f32;
            let a = self.pending[index.min(self.input_count - 1) - self.pending_start];
            let b = self.pending[(index + 1).min(self.input_count - 1) - self.pending_start];
            self.output.push(a + (b - a) * fraction);
            self.output_count += 1;
            if self.output.len() == OUTPUT_CHUNK {
                consume(&self.output)?;
                self.output.clear();
            }
        }
        if !self.output.is_empty() {
            consume(&self.output)?;
            self.output.clear();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packet_boundaries_and_tails_match_offline_bit_for_bit() {
        for rate in [8_000, 16_000, 22_050, 44_100, 48_000, 96_000, 192_000] {
            for length in (0..32).chain([479, 480, 997, 8193, 100_003]) {
                let input: Vec<f32> = (0..length)
                    .map(|i| ((i * 1777 % 65535) as f32 - 32767.0) / 32768.0)
                    .collect();
                let reference = crate::audio_dsp::resample_linear(&input, rate, TARGET_SAMPLE_RATE);
                for packet_size in [1, 3, 479, 512, 4096] {
                    let mut output = Vec::new();
                    let mut consume = |part: &[f32]| {
                        assert!(part.len() <= OUTPUT_CHUNK);
                        output.extend_from_slice(part);
                        Ok(())
                    };
                    let mut resampler = MonoResampler::new(rate).unwrap();
                    for part in input.chunks(packet_size) {
                        resampler.push(part, &mut consume).unwrap();
                        // 保留的有效输入与文件总时长无关。
                        assert!(resampler.pending.len() <= 16);
                    }
                    let count = resampler.finish(&mut consume).unwrap();
                    assert_eq!(count, reference.len() as u64);
                    assert!(
                        output
                            .iter()
                            .zip(&reference)
                            .all(|(a, b)| a.to_bits() == b.to_bits()),
                        "rate={rate}, length={length}, packet={packet_size}"
                    );
                }
            }
        }
    }

    #[test]
    fn same_rate_preserves_signed_zero_and_stops_on_consumer_error() {
        let input = [-0.0_f32, 0.0, 0.5, -0.5];
        let mut resampler = MonoResampler::new(TARGET_SAMPLE_RATE).unwrap();
        resampler
            .push(&input, &mut |part| {
                assert!(part
                    .iter()
                    .zip(input)
                    .all(|(a, b)| a.to_bits() == b.to_bits()));
                Ok(())
            })
            .unwrap();
        assert_eq!(resampler.finish(&mut |_| unreachable!()).unwrap(), 4);

        let mut calls = 0;
        let error = MonoResampler::new(48_000)
            .unwrap()
            .push(&[0.0; 32_000], &mut |_| {
                calls += 1;
                Err("cancelled".into())
            })
            .unwrap_err();
        assert_eq!(error, "cancelled");
        assert_eq!(calls, 1);
        assert!(MonoResampler::new(0).is_err());
    }
}
