//! 慢识别积压的无损临时存储。设备发送线程不执行文件 I/O。
use super::ResidentCharge;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use tokio::sync::oneshot;

const SEGMENT_BYTES: u64 = 8 * 1024 * 1024;
const DISK_BYTES: usize = 512 * 1024 * 1024;
pub(super) type Ticket = oneshot::Receiver<Result<DiskPacket, String>>;
type Failure = Arc<dyn Fn(String) + Send + Sync>;
struct Job {
    samples: Vec<f32>,
    _resident: ResidentCharge,
    reply: oneshot::Sender<Result<DiskPacket, String>>,
}
#[derive(Default)]
pub(super) struct Spool {
    sender: Mutex<Option<mpsc::Sender<Job>>>,
    disk_bytes: Arc<AtomicUsize>,
    #[cfg(test)]
    paths: Arc<Mutex<Vec<PathBuf>>>,
    #[cfg(test)]
    writer: Mutex<Option<Writer>>,
}
impl Spool {
    pub(super) fn submit(
        &self,
        samples: Vec<f32>,
        resident: ResidentCharge,
        failure: Failure,
    ) -> Result<Ticket, String> {
        let mut sender = self.sender.lock().map_err(|_| "音频暂存队列锁失败")?;
        if sender.is_none() {
            let (tx, rx) = mpsc::channel::<Job>();
            let disk_bytes = self.disk_bytes.clone();
            #[cfg(test)]
            let writer = self.writer.lock().unwrap().take().unwrap_or_default();
            #[cfg(test)]
            let paths = self.paths.clone();
            std::thread::Builder::new()
                .name("asr-audio-spool".into())
                .spawn(move || {
                    #[cfg(not(test))]
                    let writer = Writer::default();
                    #[cfg(test)]
                    let writer = Writer { paths, ..writer };
                    let mut writer = Writer {
                        disk_bytes,
                        ..writer
                    };
                    while let Ok(job) = rx.recv() {
                        if job.reply.is_closed() {
                            continue;
                        }
                        let result = writer.write(&job.samples, || job.reply.is_closed());
                        if let Err(error) = &result {
                            if !job.reply.is_closed() {
                                failure(error.clone());
                            }
                        }
                        let failed = result.is_err();
                        let _ = job.reply.send(result);
                        if failed {
                            break;
                        }
                    }
                })
                .map_err(|error| format!("启动音频暂存线程失败：{error}"))?;
            *sender = Some(tx);
        }
        let (reply, ticket) = oneshot::channel();
        sender
            .as_ref()
            .unwrap()
            .send(Job {
                samples,
                _resident: resident,
                reply,
            })
            .map_err(|_| "音频暂存线程已结束".to_string())?;
        Ok(ticket)
    }
    pub(super) fn shutdown(&self) {
        if let Ok(mut sender) = self.sender.lock() {
            sender.take();
        }
    }
}

#[cfg(test)]
impl Spool {
    pub(super) fn retained_disk_bytes(&self) -> usize {
        self.disk_bytes.load(Ordering::Acquire)
    }
    pub(super) fn test_paths(&self) -> Vec<PathBuf> {
        self.paths.lock().unwrap().clone()
    }
    pub(super) fn reject_writes_for_test(&self) {
        let path = std::env::temp_dir().join(format!(
            "say-it-asr-queue-test-{}.f32",
            uuid::Uuid::new_v4()
        ));
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        drop(file);
        let file = File::open(&path).unwrap();
        self.paths.lock().unwrap().push(path.clone());
        *self.writer.lock().unwrap() = Some(Writer {
            output: Some(Output {
                file,
                segment: Arc::new(Segment {
                    path,
                    bytes: AtomicUsize::new(0),
                    budget: self.disk_bytes.clone(),
                }),
                bytes: 0,
            }),
            ..Default::default()
        });
    }
}

struct Segment {
    path: PathBuf,
    bytes: AtomicUsize,
    budget: Arc<AtomicUsize>,
}
impl Drop for Segment {
    fn drop(&mut self) {
        self.budget
            .fetch_sub(self.bytes.load(Ordering::Acquire), Ordering::AcqRel);
        if let Err(error) = std::fs::remove_file(&self.path) {
            if error.kind() != std::io::ErrorKind::NotFound {
                eprintln!("[asr-spool] 清理临时音频失败：{error}");
            }
        }
    }
}
struct Output {
    // 先关文件，再释放拥有删除权的路径。
    file: File,
    segment: Arc<Segment>,
    bytes: u64,
}
#[derive(Default)]
struct Writer {
    output: Option<Output>,
    disk_bytes: Arc<AtomicUsize>,
    #[cfg(test)]
    paths: Arc<Mutex<Vec<PathBuf>>>,
}
impl Writer {
    fn write(
        &mut self,
        samples: &[f32],
        cancelled: impl Fn() -> bool,
    ) -> Result<DiskPacket, String> {
        let bytes = samples.len() as u64 * 4;
        if self
            .output
            .as_ref()
            .is_none_or(|output| output.bytes + bytes > SEGMENT_BYTES)
        {
            let path =
                std::env::temp_dir().join(format!("say-it-asr-queue-{}.f32", uuid::Uuid::new_v4()));
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let file = options
                .open(&path)
                .map_err(|error| format!("创建音频暂存失败：{error}"))?;
            #[cfg(test)]
            self.paths.lock().unwrap().push(path.clone());
            self.output = Some(Output {
                file,
                segment: Arc::new(Segment {
                    path,
                    bytes: AtomicUsize::new(0),
                    budget: self.disk_bytes.clone(),
                }),
                bytes: 0,
            });
        }
        let output = self.output.as_mut().unwrap();
        // 已消费的包仍可能与未消费的包共用文件；按整段实际存储计费，不能仅依赖队列长度。
        self.disk_bytes
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                used.checked_add(bytes as usize)
                    .filter(|total| *total <= DISK_BYTES)
            })
            .map_err(|_| "音频暂存磁盘预算已达上限，本次任务已停止".to_string())?;
        output
            .segment
            .bytes
            .fetch_add(bytes as usize, Ordering::AcqRel);
        let offset = output.bytes;
        let mut buffer = [0u8; 16 * 1024];
        for part in samples.chunks(buffer.len() / 4) {
            if cancelled() {
                return Err("音频暂存已取消".into());
            }
            for (sample, encoded) in part.iter().zip(buffer.chunks_exact_mut(4)) {
                encoded.copy_from_slice(&sample.to_bits().to_le_bytes());
            }
            output
                .file
                .write_all(&buffer[..part.len() * 4])
                .map_err(|error| format!("写入音频暂存失败：{error}"))?;
        }
        output.bytes += bytes;
        Ok(DiskPacket {
            segment: output.segment.clone(),
            offset,
            samples: samples.len(),
        })
    }
}
pub(super) struct DiskPacket {
    segment: Arc<Segment>,
    offset: u64,
    samples: usize,
}
struct Input {
    file: File,
    segment: Arc<Segment>,
}
#[derive(Default)]
pub(super) struct Reader {
    input: Option<Input>,
}
impl Reader {
    pub(super) fn read(&mut self, packet: DiskPacket) -> Result<Vec<f32>, String> {
        if self
            .input
            .as_ref()
            .is_none_or(|input| !Arc::ptr_eq(&input.segment, &packet.segment))
        {
            let file = File::open(&packet.segment.path)
                .map_err(|error| format!("读取音频暂存失败：{error}"))?;
            self.input = Some(Input {
                file,
                segment: packet.segment.clone(),
            });
        }
        let file = &mut self.input.as_mut().unwrap().file;
        file.seek(SeekFrom::Start(packet.offset))
            .map_err(|error| format!("定位音频暂存失败：{error}"))?;
        let mut result = Vec::with_capacity(packet.samples);
        let mut buffer = [0u8; 16 * 1024];
        while result.len() < packet.samples {
            let count = (packet.samples - result.len()).min(buffer.len() / 4);
            file.read_exact(&mut buffer[..count * 4])
                .map_err(|error| format!("音频暂存不完整：{error}"))?;
            result.extend(
                buffer[..count * 4]
                    .chunks_exact(4)
                    .map(|bytes| f32::from_bits(u32::from_le_bytes(bytes.try_into().unwrap()))),
            );
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests;
