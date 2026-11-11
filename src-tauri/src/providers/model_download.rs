use std::io::Read;
use std::path::{Path, PathBuf};

use futures_util::StreamExt;
use serde::Serialize;
use sha2::{Digest, Sha256};
use tauri::Emitter;
use tokio::io::AsyncWriteExt;

use super::plugin::{safe_model_file_path, ModelPackFileManifest, ModelPackManifest};

const PROGRESS_EVENT: &str = "model-pack-progress";

/// 正在安装或下载的模型包。同一个包同时只能有一个任务，否则两个任务会写同一个模型目录。
static ACTIVE_DOWNLOADS: std::sync::Mutex<std::collections::BTreeSet<String>> =
    std::sync::Mutex::new(std::collections::BTreeSet::new());

/// 小于这个大小的文件每次直接算，不记录：算得快，而且小文件更容易撞上同样的大小和修改时间。
const REMEMBER_MIN_BYTES: u64 = 1024 * 1024;

/// 大文件的哈希记录：（大小，修改时间）→ SHA256，存在数据目录里跨启动保留。
/// 模型权重动辄几个 GB，装好之后每次启动、每次加载都整份重读一遍没有意义：文件没被改写就沿用记录。
/// 不按路径记，安装过程中文件在暂存目录之间改名移动后记录仍然有效。
/// 代价：内容坏了但大小和修改时间没变（磁盘坏道、有人刻意改回时间）不会被发现。
#[derive(Default)]
struct HashRecords {
    file: Option<PathBuf>,
    hashes: std::collections::HashMap<(u64, u128), String>,
}

static HASH_RECORDS: std::sync::LazyLock<std::sync::Mutex<HashRecords>> =
    std::sync::LazyLock::new(Default::default);

/// 指定哈希记录存放的文件并读入已有记录。应用启动加载插件前调用一次。
pub(crate) fn remember_hashes_in(file: PathBuf) {
    let Ok(mut records) = HASH_RECORDS.lock() else {
        return;
    };
    if records.file.as_ref() == Some(&file) {
        return;
    }
    let saved: Vec<(u64, String, String)> = std::fs::read(&file)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    for (size, modified, hash) in saved {
        if let Ok(modified) = modified.parse() {
            records.hashes.insert((size, modified), hash);
        }
    }
    records.file = Some(file);
}

fn file_stamp(path: &Path) -> Result<(u64, u128), String> {
    let metadata = std::fs::metadata(path).map_err(|error| error.to_string())?;
    let modified = metadata
        .modified()
        .map_err(|error| error.to_string())?
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| error.to_string())?;
    Ok((metadata.len(), modified.as_nanos()))
}

/// 记下一个大文件的哈希（解包时边写边算出来的，或刚整份读过算出来的），之后校验它不用再读一遍。
/// 记录只是省事用的：记不下来或存不了盘都不影响校验结论，下次重新算就是了。
pub(crate) fn record_sha256(path: &Path, sha256: String) {
    let Ok(stamp) = file_stamp(path) else {
        return;
    };
    if stamp.0 < REMEMBER_MIN_BYTES {
        return;
    }
    let Ok(mut records) = HASH_RECORDS.lock() else {
        return;
    };
    records.hashes.insert(stamp, sha256);
    let Some(file) = &records.file else {
        return;
    };
    let saved = records
        .hashes
        .iter()
        .map(|((size, modified), hash)| (size, modified.to_string(), hash))
        .collect::<Vec<_>>();
    let written = serde_json::to_vec(&saved)
        .map_err(|error| error.to_string())
        .and_then(|bytes| std::fs::write(file, bytes).map_err(|error| error.to_string()));
    if let Err(error) = written {
        crate::application::diagnostics::event(
            "warn",
            "modelFiles.hashRecordsNotSaved",
            serde_json::json!({"error":error}),
        );
    }
}

/// 文件内容的 SHA256（小写十六进制）。大文件优先用记录，没有记录才整份读一遍并记下。
pub(crate) fn sha256_of(path: &Path) -> Result<String, String> {
    let stamp = file_stamp(path)?;
    if stamp.0 >= REMEMBER_MIN_BYTES {
        let records = HASH_RECORDS.lock().map_err(|_| "哈希记录锁失败")?;
        if let Some(hash) = records.hashes.get(&stamp) {
            return Ok(hash.clone());
        }
    }
    let mut input = std::fs::File::open(path).map_err(|error| error.to_string())?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        let count = input.read(&mut buffer).map_err(|error| error.to_string())?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    let hash = format!("{:x}", hasher.finalize());
    record_sha256(path, hash.clone());
    Ok(hash)
}

/// 持有期间该模型包处于「正在下载」，释放时自动解除。
pub struct DownloadLease(String);

impl Drop for DownloadLease {
    fn drop(&mut self) {
        if let Ok(mut active) = ACTIVE_DOWNLOADS.lock() {
            active.remove(&self.0);
        }
    }
}

pub fn begin_download(plugin_id: &str) -> Result<DownloadLease, String> {
    let mut active = ACTIVE_DOWNLOADS.lock().map_err(|_| "模型下载状态锁失败")?;
    if !active.insert(plugin_id.to_string()) {
        return Err("这个模型包正在下载，请等它完成".into());
    }
    Ok(DownloadLease(plugin_id.to_string()))
}

pub fn is_downloading(plugin_id: &str) -> bool {
    ACTIVE_DOWNLOADS
        .lock()
        .is_ok_and(|active| active.contains(plugin_id))
}

#[derive(Clone, Debug)]
pub struct PackInspection {
    pub state: String,
    pub total_bytes: u64,
    pub ready_bytes: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DownloadProgress<'a> {
    plugin_id: &'a str,
    file: &'a str,
    downloaded_bytes: u64,
    total_bytes: u64,
    pack_downloaded_bytes: u64,
    pack_total_bytes: u64,
    state: &'a str,
}

pub fn models_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    crate::application::data_root::data_subdir(app, "models")
}

pub fn inspect_pack(model_dir: &Path, pack: &ModelPackManifest) -> PackInspection {
    let total_bytes = pack.files.iter().map(|file| file.size_bytes).sum();
    let mut ready_bytes = 0_u64;
    let mut has_corrupt = false;
    for file in &pack.files {
        let Some(relative) = safe_model_file_path(&file.path).ok() else {
            has_corrupt = true;
            continue;
        };
        let path = model_dir.join(relative);
        if path.exists() {
            if std::fs::metadata(&path).is_ok_and(|metadata| metadata.len() == file.size_bytes) {
                ready_bytes = ready_bytes.saturating_add(file.size_bytes);
            } else {
                has_corrupt = true;
            }
        }
    }
    let has_partial = pack.files.iter().any(|file| {
        safe_model_file_path(&file.path)
            .ok()
            .is_some_and(|path| partial_path(&model_dir.join(path)).exists())
    });
    let state = if ready_bytes == total_bytes {
        "ready"
    } else if has_corrupt {
        "corrupt"
    } else if has_partial {
        "partial"
    } else {
        "pending"
    };
    PackInspection {
        state: state.into(),
        total_bytes,
        ready_bytes,
    }
}

pub fn verify_pack(model_dir: &Path, pack: &ModelPackManifest) -> Result<(), String> {
    for file in &pack.files {
        let path = model_dir.join(safe_model_file_path(&file.path)?);
        verify_model_file(&path, file)?;
    }
    Ok(())
}

/// 模型相关下载共用的客户端：只跟随 HTTPS 重定向，最多 5 跳。
/// ModelScope 对不带 User-Agent 的大文件请求返回 403（小文本文件不受限），所以必须带上。
pub fn https_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent("SayIt")
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 5 {
                attempt.error("模型下载重定向次数过多")
            } else if attempt.url().scheme() != "https" {
                attempt.error("模型下载禁止重定向到非 HTTPS 地址")
            } else {
                attempt.follow()
            }
        }))
        .build()
        .map_err(|error| error.to_string())
}

pub async fn download_pack(
    app: &tauri::AppHandle,
    plugin_id: &str,
    model_dir: &Path,
    pack: &ModelPackManifest,
) -> Result<(), String> {
    tokio::fs::create_dir_all(model_dir)
        .await
        .map_err(|error| error.to_string())?;
    let client = https_client()?;
    let pack_total = pack.files.iter().map(|file| file.size_bytes).sum();

    for file in &pack.files {
        let relative = safe_model_file_path(&file.path)?;
        let target = model_dir.join(relative);
        if target.is_file() && verify_model_file(&target, file).is_ok() {
            continue;
        }
        if target.exists() {
            tokio::fs::remove_file(&target)
                .await
                .map_err(|error| error.to_string())?;
        }
        let download = file
            .download
            .as_ref()
            .ok_or_else(|| format!("模型文件缺失且没有下载地址：{}", file.path))?;
        if let Some(parent) = target.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|error| error.to_string())?;
        }
        let pack_ready = inspect_pack(model_dir, pack).ready_bytes;
        download_file(
            &client,
            Some(app),
            plugin_id,
            &download.url,
            &target,
            file,
            pack_ready,
            pack_total,
        )
        .await?;
    }
    verify_pack(model_dir, pack)?;
    emit_progress(app, plugin_id, "", pack_total, pack_total, "ready");
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn download_file(
    client: &reqwest::Client,
    app: Option<&tauri::AppHandle>,
    plugin_id: &str,
    url: &str,
    target: &Path,
    file: &ModelPackFileManifest,
    pack_ready: u64,
    pack_total: u64,
) -> Result<(), String> {
    let partial = partial_path(target);
    let mut offset = tokio::fs::metadata(&partial)
        .await
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    if offset > file.size_bytes {
        tokio::fs::remove_file(&partial)
            .await
            .map_err(|error| error.to_string())?;
        offset = 0;
    }
    if offset == file.size_bytes && verify_model_file(&partial, file).is_ok() {
        if target.exists() {
            tokio::fs::remove_file(target)
                .await
                .map_err(|error| error.to_string())?;
        }
        return tokio::fs::rename(&partial, target)
            .await
            .map_err(|error| error.to_string());
    }
    let mut request = client.get(url);
    if offset > 0 {
        request = request.header(reqwest::header::RANGE, format!("bytes={offset}-"));
    }
    let response = request
        .send()
        .await
        .map_err(|error| format!("下载模型失败：{error}"))?;
    if !response.status().is_success() {
        return Err(format!("下载模型失败：HTTP {}", response.status()));
    }
    let resume = offset > 0 && response.status() == reqwest::StatusCode::PARTIAL_CONTENT;
    if offset > 0 && !resume {
        offset = 0;
    }
    let mut options = tokio::fs::OpenOptions::new();
    options.create(true).write(true);
    if resume {
        options.append(true);
    } else {
        options.truncate(true);
    }
    let mut output = options
        .open(&partial)
        .await
        .map_err(|error| error.to_string())?;
    let mut downloaded = offset;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| format!("下载模型中断：{error}"))?;
        output
            .write_all(&chunk)
            .await
            .map_err(|error| error.to_string())?;
        downloaded = downloaded.saturating_add(chunk.len() as u64);
        if downloaded > file.size_bytes {
            return Err(format!("下载内容超过清单大小：{}", file.path));
        }
        if let Some(app) = app {
            let _ = app.emit(
                PROGRESS_EVENT,
                DownloadProgress {
                    plugin_id,
                    file: &file.path,
                    downloaded_bytes: downloaded,
                    total_bytes: file.size_bytes,
                    pack_downloaded_bytes: pack_ready.saturating_add(downloaded),
                    pack_total_bytes: pack_total,
                    state: "downloading",
                },
            );
        }
    }
    output.flush().await.map_err(|error| error.to_string())?;
    drop(output);
    verify_model_file(&partial, file)?;
    if target.exists() {
        tokio::fs::remove_file(target)
            .await
            .map_err(|error| error.to_string())?;
    }
    tokio::fs::rename(&partial, target)
        .await
        .map_err(|error| error.to_string())
}

fn emit_progress(
    app: &tauri::AppHandle,
    plugin_id: &str,
    file: &str,
    downloaded: u64,
    total: u64,
    state: &str,
) {
    let _ = app.emit(
        PROGRESS_EVENT,
        DownloadProgress {
            plugin_id,
            file,
            downloaded_bytes: downloaded,
            total_bytes: total,
            pack_downloaded_bytes: downloaded,
            pack_total_bytes: total,
            state,
        },
    );
}

fn partial_path(target: &Path) -> PathBuf {
    let mut value = target.as_os_str().to_os_string();
    value.push(".part");
    PathBuf::from(value)
}

pub(crate) fn verify_model_file(path: &Path, file: &ModelPackFileManifest) -> Result<(), String> {
    let metadata = std::fs::metadata(path)
        .map_err(|error| format!("模型文件不存在 {}：{error}", file.path))?;
    if metadata.len() != file.size_bytes {
        return Err(format!("模型文件大小不匹配：{}", file.path));
    }
    if !sha256_of(path)?.eq_ignore_ascii_case(&file.sha256) {
        return Err(format!("模型文件 SHA256 不匹配：{}", file.path));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn same_pack_cannot_download_twice_until_lease_released() {
        let id = format!("test-{}", Uuid::new_v4());
        let lease = begin_download(&id).unwrap();
        assert!(is_downloading(&id));
        assert!(begin_download(&id).is_err());
        drop(lease);
        assert!(!is_downloading(&id));
        assert!(begin_download(&id).is_ok());
    }

    fn pack() -> ModelPackManifest {
        ModelPackManifest {
            engine: "sherpa-onnx-online".into(),
            files: vec![ModelPackFileManifest {
                path: "model.bin".into(),
                sha256: "2d711642b726b04401627ca9fbac32f5c8530fb1903cc4db02258717921a4881".into(),
                size_bytes: 1,
                download: None,
            }],
            params: serde_json::json!({}),
        }
    }

    #[test]
    fn inspection_distinguishes_ready_corrupt_and_resumable_files() {
        let root = std::env::temp_dir().join(format!("sayit-model-status-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let pack = pack();
        std::fs::write(root.join("model.bin"), b"x").unwrap();
        assert_eq!(inspect_pack(&root, &pack).state, "ready");
        std::fs::write(root.join("model.bin"), b"bad").unwrap();
        assert_eq!(inspect_pack(&root, &pack).state, "corrupt");
        std::fs::remove_file(root.join("model.bin")).unwrap();
        std::fs::write(root.join("model.bin.part"), b"").unwrap();
        assert_eq!(inspect_pack(&root, &pack).state, "partial");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn large_file_hash_is_remembered_until_the_file_is_rewritten() {
        let path = std::env::temp_dir().join(format!("sayit-hash-record-{}", Uuid::new_v4()));
        std::fs::write(&path, vec![7_u8; REMEMBER_MIN_BYTES as usize]).unwrap();
        // 记录里的值就是结论：文件没动时不再读内容。
        record_sha256(&path, "recorded".into());
        assert_eq!(sha256_of(&path).unwrap(), "recorded");
        // 移动不改变大小和修改时间，记录跟着走。
        let moved = path.with_extension("moved");
        std::fs::rename(&path, &moved).unwrap();
        assert_eq!(sha256_of(&moved).unwrap(), "recorded");
        // 改写后修改时间变了，要重新算。
        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(60);
        let file = std::fs::File::options().write(true).open(&moved).unwrap();
        file.set_modified(later).unwrap();
        drop(file);
        assert_eq!(sha256_of(&moved).unwrap().len(), 64);
        std::fs::remove_file(moved).unwrap();
    }

    #[test]
    fn rewritten_model_file_is_verified_again() {
        let path = std::env::temp_dir().join(format!("sayit-model-verify-{}", Uuid::new_v4()));
        let file = ModelPackFileManifest {
            path: "model.bin".into(),
            sha256: "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824".into(),
            size_bytes: 5,
            download: None,
        };
        std::fs::write(&path, b"hello").unwrap();
        verify_model_file(&path, &file).unwrap();
        // 同样大小的内容被改写：修改时间变了，必须重新读文件而不是沿用上次的结论。
        std::fs::write(&path, b"jello").unwrap();
        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(60);
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(later)
            .unwrap();
        assert_eq!(
            verify_model_file(&path, &file).unwrap_err(),
            "模型文件 SHA256 不匹配：model.bin"
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn downloader_resumes_with_http_range_and_verifies_hash() {
        use std::io::{Read as _, Write as _};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 2048];
            let count = stream.read(&mut request).unwrap();
            let request = String::from_utf8_lossy(&request[..count]);
            let request = request.to_ascii_lowercase();
            assert!(request.contains("range: bytes=2-"));
            assert!(request.contains("user-agent: sayit"));
            stream
                .write_all(b"HTTP/1.1 206 Partial Content\r\nContent-Length: 3\r\nContent-Range: bytes 2-4/5\r\nConnection: close\r\n\r\nllo")
                .unwrap();
        });
        let root = std::env::temp_dir().join(format!("sayit-model-resume-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let target = root.join("model.bin");
        std::fs::write(partial_path(&target), b"he").unwrap();
        let file = ModelPackFileManifest {
            path: "model.bin".into(),
            sha256: "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824".into(),
            size_bytes: 5,
            download: None,
        };
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime
            .block_on(download_file(
                &https_client().unwrap(),
                None,
                "test",
                &format!("http://{address}/model.bin"),
                &target,
                &file,
                0,
                5,
            ))
            .unwrap();
        server.join().unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"hello");
        assert!(!partial_path(&target).exists());
        std::fs::remove_dir_all(root).unwrap();
    }
}
