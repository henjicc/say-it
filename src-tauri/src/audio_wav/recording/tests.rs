use super::*;
use std::io::Cursor;

#[test]
fn incremental_wav_matches_whole_file_for_every_packet_boundary() {
    for quantization in [Quantization::Round, Quantization::Truncate] {
        for size in [0, 1, 4095, 4096, 8193, 65537] {
            let mut samples: Vec<f32> = (0..size).map(|i| (i % 997) as f32 / 397.0 - 1.2).collect();
            for (slot, value) in samples.iter_mut().zip([
                f32::NAN,
                f32::INFINITY,
                f32::NEG_INFINITY,
                -0.0,
                0.5,
                -0.5,
            ]) {
                *slot = value;
            }
            let mut expected = header(samples.len(), 44_100).unwrap().to_vec();
            super::super::write_samples(&mut expected, &samples, quantization).unwrap();
            for packet in [1, 479, 4096, 8192] {
                let mut writer =
                    StreamWriter::new(Cursor::new(Vec::new()), 44_100, quantization).unwrap();
                for part in samples.chunks(packet) {
                    writer.append(part).unwrap();
                }
                assert_eq!(writer.samples, samples.len());
                assert_eq!(writer.finish().unwrap().into_inner(), expected);
            }
        }
    }
}

#[test]
fn abandoned_and_completed_files_have_explicit_ownership() {
    let mut recording = WavRecording::new(48_000, Quantization::Round).unwrap();
    let abandoned = recording.file.as_ref().unwrap().path().to_owned();
    recording.append(&vec![0.25; 100_000]).unwrap();
    drop(recording);
    assert!(!abandoned.exists());
    let mut recording = WavRecording::new(16_000, Quantization::Round).unwrap();
    recording.append(&[0.1, -0.5, 1.0]).unwrap();
    let completed = recording.finish().unwrap();
    assert_eq!(completed.samples, 3);
    let path = completed.path().to_owned();
    assert_eq!(std::fs::metadata(&path).unwrap().len(), 50);
    drop(completed);
    assert!(!path.exists());
    let completed = WavRecording::new(16_000, Quantization::Round)
        .unwrap()
        .finish()
        .unwrap();
    let path = completed.into_path();
    assert!(path.exists(), "转移所有权后由调用方清理");
    std::fs::remove_file(path).unwrap();
}

struct FailingWriter {
    output: Cursor<Vec<u8>>,
    fail_write: bool,
    fail_seek: bool,
    fail_flush: bool,
}
impl Write for FailingWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.fail_write {
            Err(io::Error::new(io::ErrorKind::StorageFull, "full"))
        } else {
            self.output.write(bytes)
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        if self.fail_flush {
            Err(io::Error::other("flush"))
        } else {
            Ok(())
        }
    }
}
impl Seek for FailingWriter {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        if self.fail_seek {
            Err(io::Error::other("seek"))
        } else {
            self.output.seek(position)
        }
    }
}
fn writer() -> StreamWriter<FailingWriter> {
    StreamWriter::new(
        FailingWriter {
            output: Cursor::new(Vec::new()),
            fail_write: false,
            fail_seek: false,
            fail_flush: false,
        },
        48_000,
        Quantization::Round,
    )
    .unwrap()
}

#[test]
fn partial_write_seek_flush_and_size_failures_never_commit_success() {
    let mut stream = writer();
    stream.writer.fail_write = true;
    assert!(stream.append(&[0.1]).is_err());
    stream.writer.fail_write = false;
    assert!(stream.finish().is_err(), "写入失败后不能提交截断文件");
    let mut stream = writer();
    stream.append(&[0.1]).unwrap();
    stream.writer.fail_seek = true;
    assert!(stream.finish().is_err());
    let mut stream = writer();
    stream.writer.fail_flush = true;
    assert!(stream.finish().is_err());
    let mut stream = writer();
    stream.samples = (u32::MAX as usize - 36) / 2;
    assert!(stream.append(&[0.1]).is_err());
    assert!(stream.finish().is_err());
    assert!(WavRecording::new(0, Quantization::Round).is_err());
}
