use std::io::Read;
use std::path::{Path, PathBuf};

use futures_util::StreamExt;
use serde::Serialize;
use sha2::{Digest, Sha256};
use tauri::Emitter;
use tokio::io::AsyncWriteExt;

use super::plugin::{safe_model_file_path, ModelPackFileManifest, ModelPackManifest};

const PROGRESS_EVENT: &str = "model-pack-progress";

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
            if std::fs::metadata(&path)
                .is_ok_and(|metadata| metadata.len() == file.size_bytes)
            {
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

pub async fn download_pack(
    app: &tauri::AppHandle,
    plugin_id: &str,
    model_dir: &Path,
    pack: &ModelPackManifest,
) -> Result<(), String> {
    tokio::fs::create_dir_all(model_dir)
        .await
        .map_err(|error| error.to_string())?;
    let client = reqwest::Client::builder()
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
        .map_err(|error| error.to_string())?;
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
    let actual = format!("{:x}", hasher.finalize());
    if !actual.eq_ignore_ascii_case(&file.sha256) {
        return Err(format!("模型文件 SHA256 不匹配：{}", file.path));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

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
            assert!(request.to_ascii_lowercase().contains("range: bytes=2-"));
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
                &reqwest::Client::new(),
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
