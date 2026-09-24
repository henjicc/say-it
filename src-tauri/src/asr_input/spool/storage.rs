//! 文件句柄拥有暂存数据；进程退出也不依赖 Rust 析构或下一次启动扫描。
use std::fs::{File, OpenOptions};
use std::io;
use std::path::PathBuf;

pub(super) struct TemporaryFile {
    file: File,
    #[cfg(test)]
    path: PathBuf,
}

fn temporary_path() -> PathBuf {
    std::env::temp_dir().join(format!("say-it-asr-queue-{}.f32", uuid::Uuid::new_v4()))
}

fn options() -> OpenOptions {
    let mut options = OpenOptions::new();
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows::Win32::Storage::FileSystem::FILE_FLAG_DELETE_ON_CLOSE;
        // 所有读写都通过同一个受 Arc 管理的句柄；禁止其他进程重新打开原始音频。
        options
            .custom_flags(FILE_FLAG_DELETE_ON_CLOSE.0)
            .share_mode(0);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
}

impl TemporaryFile {
    pub(super) fn create() -> io::Result<Self> {
        let path = temporary_path();
        let file = options()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)?;
        Self::from_open_file(file, path)
    }

    fn from_open_file(file: File, path: PathBuf) -> io::Result<Self> {
        #[cfg(unix)]
        // 在写入任何音频前移除目录项；已打开的句柄仍可读写，进程退出时内核回收数据。
        std::fs::remove_file(&path)?;
        #[cfg(all(windows, not(test)))]
        let _ = path;
        Ok(Self {
            file,
            #[cfg(test)]
            path,
        })
    }

    pub(super) fn write_all_at(&self, mut bytes: &[u8], mut offset: u64) -> io::Result<()> {
        while !bytes.is_empty() {
            #[cfg(windows)]
            let result = {
                use std::os::windows::fs::FileExt;
                self.file.seek_write(bytes, offset)
            };
            #[cfg(unix)]
            let result = {
                use std::os::unix::fs::FileExt;
                self.file.write_at(bytes, offset)
            };
            match result {
                Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
                Ok(written) => {
                    bytes = &bytes[written..];
                    offset += written as u64;
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }

    pub(super) fn read_exact_at(&self, mut bytes: &mut [u8], mut offset: u64) -> io::Result<()> {
        while !bytes.is_empty() {
            #[cfg(windows)]
            let result = {
                use std::os::windows::fs::FileExt;
                self.file.seek_read(bytes, offset)
            };
            #[cfg(unix)]
            let result = {
                use std::os::unix::fs::FileExt;
                self.file.read_at(bytes, offset)
            };
            match result {
                Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
                Ok(read) => {
                    bytes = &mut bytes[read..];
                    offset += read as u64;
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }
}

#[cfg(test)]
impl TemporaryFile {
    pub(super) fn read_only() -> Self {
        let path = temporary_path();
        File::create_new(&path).unwrap();
        let file = options().read(true).open(&path).unwrap();
        Self::from_open_file(file, path).unwrap()
    }
    pub(super) fn path(&self) -> &std::path::Path {
        &self.path
    }
    pub(super) fn truncate(&self, len: u64) {
        self.file.set_len(len).unwrap();
    }
    pub(super) fn len(&self) -> u64 {
        self.file.metadata().unwrap().len()
    }
}
