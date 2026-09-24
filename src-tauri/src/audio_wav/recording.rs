//! 录音期间直接写最终 PCM16 文件；只有完成 WAV 头和 flush 后才交付。
use super::{encode_samples, header, Quantization};
use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Seek, SeekFrom, Write};
use std::path::PathBuf;

struct StreamWriter<W: Write + Seek> {
    writer: W,
    samples: usize,
    rate: u32,
    quantization: Quantization,
    failed: bool,
}

impl<W: Write + Seek> StreamWriter<W> {
    fn new(mut writer: W, rate: u32, quantization: Quantization) -> io::Result<Self> {
        writer.write_all(&header(0, rate)?)?;
        Ok(Self {
            writer,
            samples: 0,
            rate,
            quantization,
            failed: false,
        })
    }
    fn append(&mut self, samples: &[f32]) -> io::Result<()> {
        let result = (|| {
            if self.failed {
                return Err(io::Error::other("录音写入已失败"));
            }
            let count = self
                .samples
                .checked_add(samples.len())
                .ok_or_else(|| io::Error::other("录音过长"))?;
            header(count, self.rate)?;
            encode_samples(&mut self.writer, samples, self.quantization)?;
            self.samples = count;
            Ok(())
        })();
        self.failed |= result.is_err();
        result
    }
    fn finish(mut self) -> io::Result<W> {
        if self.failed {
            return Err(io::Error::other("录音写入已失败，不能提交部分文件"));
        }
        self.writer.seek(SeekFrom::Start(0))?;
        self.writer.write_all(&header(self.samples, self.rate)?)?;
        self.writer.flush()?;
        Ok(self.writer)
    }
}

pub(crate) struct RecordedWav {
    path: Option<PathBuf>,
    pub(crate) samples: usize,
}
impl RecordedWav {
    pub(crate) fn into_path(mut self) -> PathBuf {
        self.path.take().expect("录音路径")
    }
    #[cfg(test)]
    fn path(&self) -> &std::path::Path {
        self.path.as_deref().unwrap()
    }
}
impl Drop for RecordedWav {
    fn drop(&mut self) {
        if let Some(path) = self.path.take() {
            if let Err(error) = std::fs::remove_file(&path) {
                if error.kind() != io::ErrorKind::NotFound {
                    eprintln!("[audio-wav] 清理临时录音失败：{error}");
                }
            }
        }
    }
}

pub(crate) struct WavRecording {
    // 字段按声明顺序释放，必须先关闭 Windows 文件句柄，再清理路径。
    stream: Option<StreamWriter<BufWriter<File>>>,
    file: Option<RecordedWav>,
}
impl WavRecording {
    pub(crate) fn new(rate: u32, quantization: Quantization) -> io::Result<Self> {
        header(0, rate)?;
        let path =
            std::env::temp_dir().join(format!("say-it-dictation-{}.wav", uuid::Uuid::new_v4()));
        let output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        let file = RecordedWav {
            path: Some(path),
            samples: 0,
        };
        let stream = StreamWriter::new(
            BufWriter::with_capacity(64 * 1024, output),
            rate,
            quantization,
        )?;
        Ok(Self {
            stream: Some(stream),
            file: Some(file),
        })
    }
    pub(crate) fn append(&mut self, samples: &[f32]) -> io::Result<()> {
        self.stream.as_mut().expect("录音尚未完成").append(samples)
    }
    pub(crate) fn finish(mut self) -> io::Result<RecordedWav> {
        let stream = self.stream.take().expect("录音尚未完成");
        let count = stream.samples;
        drop(stream.finish()?);
        let mut file = self.file.take().expect("录音路径");
        file.samples = count;
        Ok(file)
    }
}

#[cfg(all(test, windows))]
mod performance_tests;
#[cfg(test)]
mod tests;
