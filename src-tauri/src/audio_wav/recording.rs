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
    /// 回写头部长度，进程中途退出时已写入的部分仍是可读的 WAV。
    fn sync_header(&mut self) -> io::Result<()> {
        self.writer.seek(SeekFrom::Start(0))?;
        self.writer.write_all(&header(self.samples, self.rate)?)?;
        self.writer.seek(SeekFrom::End(0))?;
        self.writer.flush()
    }
    /// 不论之前是否写失败，都按已成功写入的样本数提交。
    fn finish_written(mut self) -> io::Result<W> {
        self.failed = false;
        self.finish()
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
    pub(crate) fn path(&self) -> &std::path::Path {
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
        let path =
            std::env::temp_dir().join(format!("say-it-recording-{}.wav", uuid::Uuid::new_v4()));
        Self::at(path, rate, quantization)
    }
    /// 写到指定路径。完成前就被丢弃时同样删除文件；要保留需调用 `RecordedWav::into_path`。
    pub(crate) fn at(path: PathBuf, rate: u32, quantization: Quantization) -> io::Result<Self> {
        header(0, rate)?;
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
    pub(crate) fn sync_header(&mut self) -> io::Result<()> {
        self.stream.as_mut().expect("录音尚未完成").sync_header()
    }
    /// 中途写失败时也保留已写入的部分，供长录音在出错后仍交付已录下的内容。
    /// 连收尾也失败（常见于磁盘已满）时不删文件，报错里给出路径。
    pub(crate) fn finish_written(mut self) -> io::Result<RecordedWav> {
        let stream = self.stream.take().expect("录音尚未完成");
        let count = stream.samples;
        if let Err(error) = stream.finish_written() {
            let path = self.file.take().expect("录音路径").into_path();
            return Err(io::Error::other(format!("{error}（录音保留在 {}）", path.display())));
        }
        let mut file = self.file.take().expect("录音路径");
        file.samples = count;
        Ok(file)
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
