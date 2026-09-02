use hmac::{Hmac, Mac};
use rand::RngCore;
use serde::Serialize;
use serde_json::{json, Map, Value};
use sha2::Sha256;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager};
use tauri_plugin_opener::OpenerExt;
use zip::write::SimpleFileOptions;

const NORMAL_RETENTION: Duration = Duration::from_secs(7 * 24 * 60 * 60);
const CONTENT_RETENTION: Duration = Duration::from_secs(24 * 60 * 60);
const NORMAL_MAX_BYTES: u64 = 50 * 1024 * 1024;
const CONTENT_MAX_BYTES: u64 = 10 * 1024 * 1024;
const NORMAL_SEGMENT_BYTES: u64 = 5 * 1024 * 1024;
const CONTENT_SEGMENT_BYTES: u64 = 2 * 1024 * 1024;
const CONTENT_ENABLE_SECONDS: u64 = 30 * 60;

static LOGGER: OnceLock<DiagnosticLogger> = OnceLock::new();
static VERBOSE: AtomicBool = AtomicBool::new(false);

struct RollingFile {
    day: String,
    segment: u32,
    size: u64,
    file: Option<File>,
    prefix: &'static str,
}

impl RollingFile {
    fn new(prefix: &'static str) -> Self {
        Self {
            day: String::new(),
            segment: 0,
            size: 0,
            file: None,
            prefix,
        }
    }

    fn write(&mut self, directory: &Path, line: &[u8]) -> Result<(), String> {
        let day = date_key(now_seconds());
        if self.day != day || self.file.is_none() {
            self.file.take();
            self.day = day;
            self.segment = latest_segment(directory, self.prefix, &self.day);
            self.open(directory)?;
        }
        let incoming = line.len().saturating_add(1) as u64;
        let segment_limit = if self.prefix == "say-it-content" {
            CONTENT_SEGMENT_BYTES
        } else {
            NORMAL_SEGMENT_BYTES
        };
        if self.size > 0 && self.size.saturating_add(incoming) > segment_limit {
            self.file.take();
            self.segment = self.segment.saturating_add(1);
            self.open(directory)?;
        }
        let file = self.file.as_mut().ok_or("诊断日志尚未打开")?;
        file.write_all(line)
            .and_then(|_| file.write_all(b"\n"))
            .map_err(|error| format!("写入诊断日志失败：{error}"))?;
        self.size = self.size.saturating_add(incoming);
        Ok(())
    }

    fn open(&mut self, directory: &Path) -> Result<(), String> {
        if self.prefix == "say-it-content" {
            cleanup_files(
                directory,
                "say-it-content-",
                None,
                CONTENT_RETENTION,
                CONTENT_MAX_BYTES.saturating_sub(CONTENT_SEGMENT_BYTES),
            )?;
        } else {
            cleanup_files(
                directory,
                "say-it-",
                Some("say-it-content-"),
                NORMAL_RETENTION,
                NORMAL_MAX_BYTES.saturating_sub(NORMAL_SEGMENT_BYTES),
            )?;
        }
        let path = directory.join(format!(
            "{}-{}-{:03}.jsonl",
            self.prefix, self.day, self.segment
        ));
        let file = open_private_append(&path)?;
        self.size = file.metadata().map(|metadata| metadata.len()).unwrap_or(0);
        self.file = Some(file);
        Ok(())
    }

    fn close(&mut self) {
        self.file.take();
        self.day.clear();
        self.segment = 0;
        self.size = 0;
    }
}

enum LogCommand {
    Normal(Vec<u8>),
    Content(Vec<u8>),
    CloseContent,
    Flush(mpsc::Sender<()>),
    Clear(mpsc::Sender<Result<(), String>>),
}

struct DiagnosticLogger {
    directory: PathBuf,
    version: String,
    fingerprint_key: [u8; 32],
    sender: mpsc::Sender<LogCommand>,
    content_deadline: AtomicU64,
    content_generation: AtomicU64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DiagnosticStatus {
    pub(crate) directory: String,
    pub(crate) verbose_logging: bool,
    pub(crate) content_logging_enabled: bool,
    pub(crate) content_logging_remaining_seconds: u64,
}

pub(crate) fn initialize(app: &AppHandle) -> Result<(), String> {
    let directory = app
        .path()
        .app_log_dir()
        .map_err(|error| format!("定位应用日志目录失败：{error}"))?;
    std::fs::create_dir_all(&directory)
        .map_err(|error| format!("创建应用日志目录失败：{error}"))?;
    let mut key = [0u8; 32];
    rand::rng().fill_bytes(&mut key);
    cleanup_files(
        &directory,
        "say-it-",
        Some("say-it-content-"),
        NORMAL_RETENTION,
        NORMAL_MAX_BYTES,
    )?;
    cleanup_files(
        &directory,
        "say-it-content-",
        None,
        CONTENT_RETENTION,
        CONTENT_MAX_BYTES,
    )?;
    let (sender, receiver) = mpsc::channel();
    let worker_directory = directory.clone();
    std::thread::Builder::new()
        .name("sayit-diagnostics".into())
        .spawn(move || run_writer(worker_directory, receiver))
        .map_err(|error| format!("启动诊断日志写入器失败：{error}"))?;
    let logger = DiagnosticLogger {
        directory,
        version: app.package_info().version.to_string(),
        fingerprint_key: key,
        sender,
        content_deadline: AtomicU64::new(0),
        content_generation: AtomicU64::new(0),
    };
    let _ = LOGGER.set(logger);
    event(
        "info",
        "diagnostics.initialized",
        json!({"platform": std::env::consts::OS}),
    );
    Ok(())
}

pub(crate) fn set_verbose(enabled: bool) {
    VERBOSE.store(enabled, Ordering::Relaxed);
    event(
        "info",
        "diagnostics.verboseChanged",
        json!({"enabled":enabled}),
    );
}

pub(crate) fn verbose_enabled() -> bool {
    VERBOSE.load(Ordering::Relaxed)
}

pub(crate) fn fingerprint(text: &str) -> String {
    let Some(logger) = LOGGER.get() else {
        return String::new();
    };
    let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(&logger.fingerprint_key) else {
        return String::new();
    };
    mac.update(text.as_bytes());
    mac.finalize()
        .into_bytes()
        .iter()
        .take(6)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub(crate) fn event(level: &str, name: &str, metadata: Value) {
    if level == "debug" && !verbose_enabled() {
        return;
    }
    let Some(logger) = LOGGER.get() else {
        return;
    };
    let record = json!({
        "timestampMs": now_millis(),
        "level": level,
        "event": name,
        "version": logger.version,
        "platform": std::env::consts::OS,
        "metadata": sanitize(metadata),
    });
    let Ok(line) = serde_json::to_vec(&record) else {
        return;
    };
    let _ = logger.sender.send(LogCommand::Normal(line));
}

pub(crate) fn content_event(name: &str, content: Value) {
    let Some(logger) = LOGGER.get() else {
        return;
    };
    if !content_enabled(logger) {
        return;
    }
    let record = json!({
        "timestampMs": now_millis(),
        "level": "content",
        "event": name,
        "platform": std::env::consts::OS,
        "warning": "containsUserText",
        "content": content,
    });
    let Ok(line) = serde_json::to_vec(&record) else {
        return;
    };
    let _ = logger.sender.send(LogCommand::Content(line));
}

pub(crate) fn legacy_debug_log(component: &str, message: &str) {
    event(
        "debug",
        "legacy.debug",
        json!({
            "component":component,
            "detailChars":message.chars().count(),
            "detailFingerprint":fingerprint(message),
        }),
    );
    content_event(
        "legacy.debug",
        json!({"component":component,"message":message}),
    );
}

#[tauri::command]
pub(crate) fn get_diagnostic_status() -> Result<DiagnosticStatus, String> {
    status()
}

#[tauri::command]
pub(crate) fn set_content_diagnostics(enabled: bool) -> Result<DiagnosticStatus, String> {
    let logger = LOGGER.get().ok_or("诊断日志尚未初始化")?;
    let generation = logger.content_generation.fetch_add(1, Ordering::AcqRel) + 1;
    if enabled {
        logger.content_deadline.store(
            now_seconds().saturating_add(CONTENT_ENABLE_SECONDS),
            Ordering::Release,
        );
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(Duration::from_secs(CONTENT_ENABLE_SECONDS)).await;
            if let Some(logger) = LOGGER.get() {
                if logger.content_generation.load(Ordering::Acquire) == generation {
                    logger.content_deadline.store(0, Ordering::Release);
                    let _ = logger.sender.send(LogCommand::CloseContent);
                    event("info", "diagnostics.contentExpired", json!({}));
                }
            }
        });
    } else {
        logger.content_deadline.store(0, Ordering::Release);
        let _ = logger.sender.send(LogCommand::CloseContent);
    }
    event(
        "info",
        "diagnostics.contentChanged",
        json!({"enabled":enabled}),
    );
    status()
}

#[tauri::command]
pub(crate) fn clear_diagnostic_logs() -> Result<(), String> {
    let logger = LOGGER.get().ok_or("诊断日志尚未初始化")?;
    let (sender, receiver) = mpsc::channel();
    logger
        .sender
        .send(LogCommand::Clear(sender))
        .map_err(|_| "诊断日志写入器已停止")?;
    receiver.recv().map_err(|_| "诊断日志写入器未响应")??;
    event("info", "diagnostics.cleared", json!({}));
    Ok(())
}

#[tauri::command]
pub(crate) fn open_diagnostic_directory(app: AppHandle) -> Result<(), String> {
    let logger = LOGGER.get().ok_or("诊断日志尚未初始化")?;
    flush(logger)?;
    app.opener()
        .open_path(
            logger.directory.to_string_lossy().into_owned(),
            None::<&str>,
        )
        .map_err(|error| format!("打开日志目录失败：{error}"))
}

#[tauri::command]
pub(crate) fn export_diagnostic_bundle(
    app: AppHandle,
    destination: String,
    include_content: bool,
) -> Result<(), String> {
    let logger = LOGGER.get().ok_or("诊断日志尚未初始化")?;
    let destination = PathBuf::from(destination.trim());
    if !destination.is_absolute() {
        return Err("诊断包保存路径必须是绝对路径".into());
    }
    let file = File::create(&destination).map_err(|error| format!("创建诊断包失败：{error}"))?;
    let mut archive = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    archive
        .start_file("summary.json", options)
        .map_err(|error| format!("创建诊断摘要失败：{error}"))?;
    let summary = json!({
        "version": app.package_info().version.to_string(),
        "platform": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "contentIncluded": include_content,
        "exportedAtMs": now_millis(),
    });
    archive
        .write_all(
            serde_json::to_string_pretty(&summary)
                .unwrap_or_default()
                .as_bytes(),
        )
        .map_err(|error| format!("写入诊断摘要失败：{error}"))?;
    archive
        .start_file("configuration.json", options)
        .map_err(|error| format!("创建配置投影失败：{error}"))?;
    let configuration = app
        .state::<crate::state::RuntimeState>()
        .app_settings
        .lock()
        .map(|settings| json!({
            "historyEnabled": settings.history_prefs.get("enabled").and_then(Value::as_bool).unwrap_or(true),
            "finalDraftObservationEnabled": settings.history_prefs.get("finalDraftObservationEnabled").and_then(Value::as_bool).unwrap_or(false),
            "correctionLearningEnabled": settings.history_prefs.get("correctionLearningEnabled").and_then(Value::as_bool).unwrap_or(false),
            "cloudLearningContextEnabled": settings.history_prefs.get("cloudLearningContextEnabled").and_then(Value::as_bool).unwrap_or(false),
            "learningMemoryRetentionDays": settings.history_prefs.get("learningMemoryRetentionDays").and_then(Value::as_u64).unwrap_or(180),
            "historyRetentionDays": settings.history_prefs.get("retentionDays").and_then(Value::as_u64).unwrap_or(30),
            "verboseLogging": settings.diagnostics_prefs.get("verboseLogging").and_then(Value::as_bool).unwrap_or(false),
        }))
        .unwrap_or_else(|_| json!({"unavailable":true}));
    archive
        .write_all(
            serde_json::to_string_pretty(&configuration)
                .unwrap_or_default()
                .as_bytes(),
        )
        .map_err(|error| format!("写入配置投影失败：{error}"))?;
    for entry in std::fs::read_dir(&logger.directory)
        .map_err(|error| format!("读取日志目录失败：{error}"))?
    {
        let entry = entry.map_err(|error| format!("读取日志文件失败：{error}"))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let is_normal = name.starts_with("say-it-")
            && !name.starts_with("say-it-content-")
            && name.ends_with(".jsonl");
        let is_content =
            include_content && name.starts_with("say-it-content-") && name.ends_with(".jsonl");
        let is_stack_overflow = name == "say-it-stack-overflow.log";
        if !is_normal && !is_content && !is_stack_overflow {
            continue;
        }
        let mut source =
            File::open(entry.path()).map_err(|error| format!("打开待导出日志失败：{error}"))?;
        archive
            .start_file(format!("logs/{name}"), options)
            .map_err(|error| format!("创建诊断包日志条目失败：{error}"))?;
        std::io::copy(&mut source, &mut archive)
            .map_err(|error| format!("导出日志失败：{error}"))?;
    }
    archive
        .finish()
        .map_err(|error| format!("完成诊断包失败：{error}"))?;
    Ok(())
}

fn status() -> Result<DiagnosticStatus, String> {
    let logger = LOGGER.get().ok_or("诊断日志尚未初始化")?;
    let now = now_seconds();
    let deadline = logger.content_deadline.load(Ordering::Acquire);
    Ok(DiagnosticStatus {
        directory: logger.directory.display().to_string(),
        verbose_logging: verbose_enabled(),
        content_logging_enabled: deadline > now,
        content_logging_remaining_seconds: deadline.saturating_sub(now),
    })
}

fn content_enabled(logger: &DiagnosticLogger) -> bool {
    logger.content_deadline.load(Ordering::Acquire) > now_seconds()
}

fn flush(logger: &DiagnosticLogger) -> Result<(), String> {
    let (sender, receiver) = mpsc::channel();
    logger
        .sender
        .send(LogCommand::Flush(sender))
        .map_err(|_| "诊断日志写入器已停止")?;
    receiver
        .recv()
        .map_err(|_| "诊断日志写入器未响应".to_string())
}

fn run_writer(directory: PathBuf, receiver: mpsc::Receiver<LogCommand>) {
    let mut normal = RollingFile::new("say-it");
    let mut content = RollingFile::new("say-it-content");
    while let Ok(command) = receiver.recv() {
        match command {
            LogCommand::Normal(line) => {
                let _ = normal.write(&directory, &line);
            }
            LogCommand::Content(line) => {
                let _ = content.write(&directory, &line);
            }
            LogCommand::CloseContent => content.close(),
            LogCommand::Flush(response) => {
                if let Some(file) = normal.file.as_mut() {
                    let _ = file.flush();
                }
                if let Some(file) = content.file.as_mut() {
                    let _ = file.flush();
                }
                let _ = response.send(());
            }
            LogCommand::Clear(response) => {
                normal.close();
                content.close();
                let result = clear_files(&directory);
                let _ = response.send(result);
            }
        }
    }
}

fn clear_files(directory: &Path) -> Result<(), String> {
    for entry in
        std::fs::read_dir(directory).map_err(|error| format!("读取日志目录失败：{error}"))?
    {
        let entry = entry.map_err(|error| format!("读取日志文件失败：{error}"))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("say-it-") && name.ends_with(".jsonl") {
            std::fs::remove_file(entry.path()).map_err(|error| format!("删除日志失败：{error}"))?;
        }
    }
    Ok(())
}

fn latest_segment(directory: &Path, prefix: &str, day: &str) -> u32 {
    let stem = format!("{prefix}-{day}-");
    std::fs::read_dir(directory)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            name.strip_prefix(&stem)
                .and_then(|suffix| suffix.strip_suffix(".jsonl"))
                .and_then(|segment| segment.parse::<u32>().ok())
        })
        .max()
        .unwrap_or(0)
}

fn open_private_append(path: &Path) -> Result<File, String> {
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options
        .open(path)
        .map_err(|error| format!("打开诊断日志失败：{error}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .map_err(|error| format!("设置诊断日志权限失败：{error}"))?;
    }
    Ok(file)
}

fn sanitize(value: Value) -> Value {
    match value {
        Value::Object(values) => Value::Object(
            values
                .into_iter()
                .map(|(key, value)| {
                    let normalized = key.to_ascii_lowercase();
                    let allowed_summary = normalized.ends_with("chars")
                        || normalized.ends_with("bytes")
                        || normalized.contains("fingerprint");
                    let sensitive = !allowed_summary
                        && (matches!(normalized.as_str(), "message" | "detail" | "error")
                            || [
                                "text",
                                "prompt",
                                "secret",
                                "token",
                                "apikey",
                                "api_key",
                                "clipboard",
                                "windowtitle",
                                "window_title",
                            ]
                            .iter()
                            .any(|needle| normalized.contains(needle)));
                    (
                        key,
                        if sensitive {
                            Value::String("[redacted]".into())
                        } else {
                            sanitize(value)
                        },
                    )
                })
                .collect::<Map<_, _>>(),
        ),
        Value::Array(values) => Value::Array(values.into_iter().map(sanitize).collect()),
        other => other,
    }
}

fn cleanup_files(
    directory: &Path,
    prefix: &str,
    excluded_prefix: Option<&str>,
    retention: Duration,
    max_bytes: u64,
) -> Result<(), String> {
    let now = SystemTime::now();
    let mut files = Vec::new();
    for entry in
        std::fs::read_dir(directory).map_err(|error| format!("读取日志目录失败：{error}"))?
    {
        let entry = entry.map_err(|error| format!("读取日志文件失败：{error}"))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with(prefix)
            || excluded_prefix.is_some_and(|excluded| name.starts_with(excluded))
            || !name.ends_with(".jsonl")
        {
            continue;
        }
        let metadata = entry
            .metadata()
            .map_err(|error| format!("读取日志元数据失败：{error}"))?;
        let modified = metadata.modified().unwrap_or(UNIX_EPOCH);
        if now.duration_since(modified).unwrap_or_default() > retention {
            let _ = std::fs::remove_file(entry.path());
        } else {
            files.push((entry.path(), modified, metadata.len()));
        }
    }
    files.sort_by_key(|(_, modified, _)| *modified);
    let mut total = files.iter().map(|(_, _, size)| *size).sum::<u64>();
    for (path, _, size) in files {
        if total <= max_bytes {
            break;
        }
        if std::fs::remove_file(path).is_ok() {
            total = total.saturating_sub(size);
        }
    }
    Ok(())
}

fn now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn date_key(seconds: u64) -> String {
    let days = (seconds / 86_400) as i64;
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizer_removes_text_and_secret_fields_but_keeps_summaries() {
        let value = sanitize(json!({
            "rawText":"secret sentence",
            "message":"another secret sentence",
            "apiKey":"token",
            "textChars":15,
            "textFingerprint":"abc",
            "status":"ok"
        }));
        assert_eq!(value["rawText"], "[redacted]");
        assert_eq!(value["apiKey"], "[redacted]");
        assert_eq!(value["message"], "[redacted]");
        assert_eq!(value["textChars"], 15);
        assert_eq!(value["status"], "ok");
        assert!(!value.to_string().contains("secret sentence"));
    }

    #[test]
    fn date_key_matches_unix_epoch() {
        assert_eq!(date_key(0), "1970-01-01");
    }

    #[test]
    fn normal_log_cleanup_never_counts_or_deletes_content_logs() {
        let directory =
            std::env::temp_dir().join(format!("sayit-diagnostics-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let normal = directory.join("say-it-2026-01-01.jsonl");
        let content = directory.join("say-it-content-2026-01-01.jsonl");
        std::fs::write(&normal, b"normal").unwrap();
        std::fs::write(&content, b"content").unwrap();
        cleanup_files(
            &directory,
            "say-it-",
            Some("say-it-content-"),
            Duration::from_secs(u64::MAX),
            0,
        )
        .unwrap();
        assert!(!normal.exists());
        assert!(content.exists());
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn rolling_writer_can_be_closed_cleared_and_recreated() {
        let directory =
            std::env::temp_dir().join(format!("sayit-diagnostics-writer-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let mut writer = RollingFile::new("say-it");
        writer.write(&directory, b"first").unwrap();
        let path = directory.join(format!("say-it-{}-000.jsonl", date_key(now_seconds())));
        writer.close();
        std::fs::remove_file(&path).unwrap();
        writer.write(&directory, b"second").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "second\n");
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn rolling_writer_uses_private_permissions_on_unix() {
        let directory =
            std::env::temp_dir().join(format!("sayit-diagnostics-cap-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let mut writer = RollingFile::new("say-it");
        writer.write(&directory, b"private").unwrap();
        let current = directory.join(format!("say-it-{}-000.jsonl", date_key(now_seconds())));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                current.metadata().unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        let _ = std::fs::remove_dir_all(directory);
    }
}
