//! 固定内存的单声道 PCM16 WAV 写出器。量化方式由原有调用方契约决定。
use std::io::{self, BufWriter, Write};
use std::path::Path;

const CHUNK_SAMPLES: usize = 8_192;

#[derive(Clone, Copy)]
pub(crate) enum Quantization {
    Truncate,
    Round,
}

fn header(samples: usize, rate: u32) -> io::Result<[u8; 44]> {
    let invalid = || {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "采样率无效或音频超过 WAV 格式上限",
        )
    };
    let data_len = samples
        .checked_mul(2)
        .and_then(|n| u32::try_from(n).ok())
        .ok_or_else(invalid)?;
    let riff_len = data_len.checked_add(36).ok_or_else(invalid)?;
    let byte_rate = rate
        .checked_mul(2)
        .filter(|n| *n != 0)
        .ok_or_else(invalid)?;
    let mut bytes = [0u8; 44];
    bytes[..4].copy_from_slice(b"RIFF");
    bytes[4..8].copy_from_slice(&riff_len.to_le_bytes());
    bytes[8..16].copy_from_slice(b"WAVEfmt ");
    bytes[16..20].copy_from_slice(&16u32.to_le_bytes());
    bytes[20..22].copy_from_slice(&1u16.to_le_bytes());
    bytes[22..24].copy_from_slice(&1u16.to_le_bytes());
    bytes[24..28].copy_from_slice(&rate.to_le_bytes());
    bytes[28..32].copy_from_slice(&byte_rate.to_le_bytes());
    bytes[32..34].copy_from_slice(&2u16.to_le_bytes());
    bytes[34..36].copy_from_slice(&16u16.to_le_bytes());
    bytes[36..40].copy_from_slice(b"data");
    bytes[40..44].copy_from_slice(&data_len.to_le_bytes());
    Ok(bytes)
}

fn write_samples(
    writer: &mut impl Write,
    samples: &[f32],
    quantization: Quantization,
) -> io::Result<()> {
    let mut buffer = [0u8; CHUNK_SAMPLES * 2];
    for chunk in samples.chunks(CHUNK_SAMPLES) {
        for (sample, bytes) in chunk.iter().zip(buffer.chunks_exact_mut(2)) {
            let value = sample.clamp(-1.0, 1.0) * i16::MAX as f32;
            let value = match quantization {
                Quantization::Truncate => value as i16,
                Quantization::Round => value.round() as i16,
            };
            bytes.copy_from_slice(&value.to_le_bytes());
        }
        writer.write_all(&buffer[..chunk.len() * 2])?;
    }
    writer.flush()
}

pub(crate) fn write_mono_pcm16(
    path: &Path,
    samples: &[f32],
    rate: u32,
    quantization: Quantization,
) -> io::Result<()> {
    // 先校验，格式溢出时不得截断调用方的现有文件。
    let header = header(samples.len(), rate)?;
    let file = std::fs::File::create(path)?;
    let mut writer = BufWriter::with_capacity(64 * 1024, file);
    writer.write_all(&header)?;
    write_samples(&mut writer, samples, quantization)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_boundaries_and_quantization_preserve_existing_pcm() {
        let edges = [
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
            -0.0,
            0.0,
            0.5,
            -0.5,
        ];
        for length in [0, 1, 8191, 8192, 8193, 65537] {
            let samples: Vec<f32> = (0..length)
                .map(|i| {
                    if i < edges.len() {
                        edges[i]
                    } else {
                        (i % 997) as f32 / 398.0 - 1.2
                    }
                })
                .collect();
            for quantization in [Quantization::Truncate, Quantization::Round] {
                let mut output = Vec::new();
                write_samples(&mut output, &samples, quantization).unwrap();
                let expected: Vec<u8> = match quantization {
                    Quantization::Truncate => samples
                        .iter()
                        .map(|s| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
                        .collect::<Vec<_>>(),
                    Quantization::Round => crate::audio_prep::f32_to_i16(&samples),
                }
                .into_iter()
                .flat_map(i16::to_le_bytes)
                .collect();
                assert_eq!(output, expected);
            }
        }
    }

    #[test]
    fn riff_and_rate_overflow_are_rejected_before_opening_a_file() {
        assert!(header(0, 0).is_err());
        assert!(header(0, u32::MAX).is_err());
        assert!(header(usize::MAX, 48000).is_err());
        assert!(header(u32::MAX as usize / 2, 48000).is_err());
        assert!(header((u32::MAX as usize - 36) / 2, 48000).is_ok());
        let path =
            std::env::temp_dir().join(format!("say-it-wav-guard-{}.wav", uuid::Uuid::new_v4()));
        std::fs::write(&path, b"previous").unwrap();
        assert!(write_mono_pcm16(&path, &[0.0], 0, Quantization::Truncate).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"previous");
        std::fs::remove_file(path).unwrap();
    }

    struct BoundedSink {
        bytes: usize,
        fail_after: Option<usize>,
        fail_flush: bool,
    }
    impl Write for BoundedSink {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            assert!(bytes.len() <= CHUNK_SAMPLES * 2);
            if self.fail_after.is_some_and(|limit| self.bytes >= limit) {
                return Err(io::Error::new(io::ErrorKind::StorageFull, "test disk full"));
            }
            self.bytes += bytes.len();
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            if self.fail_flush {
                Err(io::Error::other("test flush failure"))
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn writes_are_bounded_and_disk_errors_propagate() {
        let samples = vec![0.1; CHUNK_SAMPLES * 3 + 1];
        let mut sink = BoundedSink {
            bytes: 0,
            fail_after: None,
            fail_flush: false,
        };
        write_samples(&mut sink, &samples, Quantization::Truncate).unwrap();
        assert_eq!(sink.bytes, samples.len() * 2);
        sink.bytes = 0;
        sink.fail_after = Some(CHUNK_SAMPLES * 2);
        assert_eq!(
            write_samples(&mut sink, &samples, Quantization::Truncate)
                .unwrap_err()
                .kind(),
            io::ErrorKind::StorageFull
        );
        sink.fail_after = None;
        sink.fail_flush = true;
        assert!(write_samples(&mut sink, &samples, Quantization::Round).is_err());
    }
}
