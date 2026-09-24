//! 私有浮点暂存由文件句柄持有；可按路径读取的试听文件按平台处理删除寿命。
use std::fs::{File, OpenOptions};
use std::io;
use std::path::PathBuf;

pub(crate) struct TemporaryFile {
    file: File,
    readable_path: Option<PathBuf>,
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
    // 试听协议需要按路径打开文件；Windows 只额外允许读和共享删除，不允许其他写入者。
    pub(crate) fn create_readable() -> io::Result<Self> {
        let path =
            std::env::temp_dir().join(format!("say-it-audio-lab-{}.wav", uuid::Uuid::new_v4()));
        let mut options = options();
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            use windows::Win32::Storage::FileSystem::{FILE_SHARE_DELETE, FILE_SHARE_READ};
            options.share_mode(FILE_SHARE_READ.0 | FILE_SHARE_DELETE.0);
        }
        let file = options
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)?;
        Ok(Self {
            file,
            readable_path: Some(path.clone()),
            #[cfg(test)]
            path,
        })
    }
    pub(crate) fn readable_path(&self) -> &std::path::Path {
        self.readable_path.as_deref().expect("试听文件才公开路径")
    }
    pub(crate) fn writer(&self) -> &File {
        &self.file
    }
    pub(crate) fn create() -> io::Result<Self> {
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
            readable_path: None,
            #[cfg(test)]
            path,
        })
    }

    pub(crate) fn write_all_at(&self, mut bytes: &[u8], mut offset: u64) -> io::Result<()> {
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

    pub(crate) fn read_exact_at(&self, mut bytes: &mut [u8], mut offset: u64) -> io::Result<()> {
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

#[cfg(unix)]
impl Drop for TemporaryFile {
    fn drop(&mut self) {
        if let Some(path) = &self.readable_path {
            if let Err(error) = std::fs::remove_file(path) {
                if error.kind() != io::ErrorKind::NotFound {
                    eprintln!("清理试听文件失败：{error}");
                }
            }
        }
    }
}

#[cfg(test)]
impl TemporaryFile {
    pub(crate) fn read_only() -> Self {
        let path = temporary_path();
        File::create_new(&path).unwrap();
        let file = options().read(true).open(&path).unwrap();
        Self::from_open_file(file, path).unwrap()
    }
    pub(crate) fn path(&self) -> &std::path::Path {
        &self.path
    }
    pub(crate) fn truncate(&self, len: u64) {
        self.file.set_len(len).unwrap();
    }
    pub(crate) fn len(&self) -> u64 {
        self.file.metadata().unwrap().len()
    }
}
