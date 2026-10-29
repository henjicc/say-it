//! 短素材保留内存，长素材保留原始 f32 位值并由文件句柄管理寿命。
use crate::temporary_audio::TemporaryFile;
use std::io;
use std::sync::{Arc, Mutex};

const MEMORY_SAMPLES: usize = 2 * 1024 * 1024 / 4;
const MAX_SAMPLES: usize = 512 * 1024 * 1024 / 4;
pub(crate) const BLOCK: usize = 8192;

#[derive(Clone)]
enum Storage {
    Memory(Arc<Vec<f32>>),
    Disk(Arc<TemporaryFile>),
}
pub(crate) struct AudioBuffer {
    storage: Storage,
    len: usize,
    waveform: Mutex<Option<(usize, Vec<[f32; 2]>)>>,
}
impl Default for AudioBuffer {
    fn default() -> Self {
        Self {
            storage: Storage::Memory(Arc::new(Vec::new())),
            len: 0,
            waveform: Mutex::new(None),
        }
    }
}
impl AudioBuffer {
    // 原素材只追加；已存在的文件区间不变，内存分支写时复制，因此处理快照不阻塞录音或新会话。
    pub(crate) fn snapshot(&self) -> Self {
        Self {
            storage: self.storage.clone(),
            len: self.len,
            waveform: Mutex::new(None),
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.len
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.len == 0
    }
    pub(crate) fn append(&mut self, input: &[f32]) -> io::Result<()> {
        let next = self
            .len
            .checked_add(input.len())
            .filter(|n| *n <= MAX_SAMPLES)
            .ok_or_else(|| io::Error::other("音频素材已达暂存容量上限，请缩短录音"))?;
        if next > MEMORY_SAMPLES {
            if let Storage::Memory(samples) = &self.storage {
                // 迁移成功后才替换原存储；失败时保留已有完整素材。
                let file = TemporaryFile::create()?;
                write_samples(&file, 0, samples)?;
                self.storage = Storage::Disk(Arc::new(file));
            }
        }
        match &mut self.storage {
            Storage::Memory(samples) => {
                let samples = Arc::make_mut(samples);
                if next > samples.capacity() {
                    let capacity = next.max(samples.capacity() * 2).min(MEMORY_SAMPLES);
                    samples.reserve_exact(capacity - samples.len());
                }
                samples.extend_from_slice(input);
            }
            Storage::Disk(file) => write_samples(file, self.len, input)?,
        }
        self.len = next;
        *self
            .waveform
            .get_mut()
            .map_err(|_| io::Error::other("波形缓存锁失败"))? = None;
        Ok(())
    }
    pub(crate) fn read(&self, start: usize, output: &mut [f32]) -> io::Result<()> {
        if start
            .checked_add(output.len())
            .is_none_or(|end| end > self.len)
        {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        match &self.storage {
            Storage::Memory(samples) => {
                output.copy_from_slice(&samples[start..start + output.len()])
            }
            Storage::Disk(file) => {
                let mut bytes = [0u8; BLOCK * 4];
                for (index, chunk) in output.chunks_mut(BLOCK).enumerate() {
                    file.read_exact_at(
                        &mut bytes[..chunk.len() * 4],
                        ((start + index * BLOCK) * 4) as u64,
                    )?;
                    for (sample, encoded) in chunk.iter_mut().zip(bytes.chunks_exact(4)) {
                        *sample = f32::from_le_bytes(encoded.try_into().unwrap());
                    }
                }
            }
        }
        Ok(())
    }
    pub(crate) fn replace(&mut self, start: usize, input: &[f32]) -> io::Result<()> {
        if start
            .checked_add(input.len())
            .is_none_or(|end| end > self.len)
        {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        *self
            .waveform
            .get_mut()
            .map_err(|_| io::Error::other("波形缓存锁失败"))? = None;
        match &mut self.storage {
            Storage::Memory(samples) => {
                Arc::make_mut(samples)[start..start + input.len()].copy_from_slice(input)
            }
            Storage::Disk(file) => {
                if Arc::strong_count(file) != 1 {
                    return Err(io::Error::other("共享音频素材不能就地改写"));
                }
                write_samples(file, start, input)?;
            }
        }
        Ok(())
    }
    pub(crate) fn visit(&self, mut action: impl FnMut(&[f32]) -> io::Result<()>) -> io::Result<()> {
        let mut block = [0f32; BLOCK];
        for start in (0..self.len).step_by(BLOCK) {
            let count = (self.len - start).min(BLOCK);
            self.read(start, &mut block[..count])?;
            action(&block[..count])?;
        }
        Ok(())
    }
    pub(crate) fn waveform(&self, points: usize) -> io::Result<Vec<[f32; 2]>> {
        let mut cache = self
            .waveform
            .lock()
            .map_err(|_| io::Error::other("波形缓存锁失败"))?;
        if let Some((width, waveform)) = &*cache {
            if *width == points {
                return Ok(waveform.clone());
            }
        }
        let width = points.min(self.len);
        let mut result = vec![[1.0f32, -1.0f32]; width];
        let mut offset = 0;
        let mut bin = 0;
        if width > 0 {
            self.visit(|mut samples| {
                while !samples.is_empty() {
                    let end = (bin + 1) * self.len / width;
                    let count = (end - offset).min(samples.len());
                    result[bin] = samples[..count]
                        .iter()
                        .fold(result[bin], |[min, max], sample| {
                            [min.min(*sample), max.max(*sample)]
                        });
                    offset += count;
                    samples = &samples[count..];
                    if offset == end {
                        bin += 1;
                    }
                }
                Ok(())
            })?;
        }
        *cache = Some((points, result.clone()));
        Ok(result)
    }
    #[cfg(test)]
    pub(crate) fn is_released(&self) -> bool {
        self.len == 0
            && matches!(&self.storage, Storage::Memory(samples) if samples.capacity() == 0)
    }
    #[cfg(test)]
    pub(crate) fn to_vec(&self) -> Vec<f32> {
        let mut output = vec![0.0; self.len];
        self.read(0, &mut output).unwrap();
        output
    }
}
fn write_samples(file: &TemporaryFile, start: usize, samples: &[f32]) -> io::Result<()> {
    let mut bytes = [0u8; BLOCK * 4];
    for (index, chunk) in samples.chunks(BLOCK).enumerate() {
        for (sample, encoded) in chunk.iter().zip(bytes.chunks_exact_mut(4)) {
            encoded.copy_from_slice(&sample.to_le_bytes());
        }
        file.write_all_at(
            &bytes[..chunk.len() * 4],
            ((start + index * BLOCK) * 4) as u64,
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
