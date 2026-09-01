use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager};
use uuid::Uuid;

const HISTORY_FILE: &str = "history.sqlite3";
const RECOVERY_NOTICE_FILE: &str = "history-recovery.json";
const DEFAULT_RETENTION_DAYS: u32 = 30;
const MAX_PAGE_SIZE: u32 = 100;
pub(crate) const HISTORY_EVENT: &str = "history-changed";
pub(crate) const OPEN_HISTORY_EVENT: &str = "open-history";

static WRITES_PAUSED: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HistoryEntry {
    pub(crate) id: String,
    pub(crate) created_at: i64,
    pub(crate) task_kind: String,
    pub(crate) source_text: String,
    pub(crate) output_text: String,
    pub(crate) final_text: Option<String>,
    #[serde(skip)]
    pub(crate) final_text_baseline: Option<String>,
    pub(crate) final_text_confidence: Option<String>,
    pub(crate) final_text_source: Option<String>,
    pub(crate) final_text_observed_at: Option<i64>,
    pub(crate) smart_processing_applied: bool,
    pub(crate) learning_status: String,
    pub(crate) correction_kind: Option<String>,
    pub(crate) learning_scope: Option<String>,
    pub(crate) applied_rule_ids: Vec<String>,
    pub(crate) diff_segments: Vec<TextDiffSegment>,
    pub(crate) instruction: String,
    pub(crate) app_name: String,
    pub(crate) process_name: String,
    pub(crate) provider_id: String,
    pub(crate) model_id: String,
    pub(crate) status: String,
    pub(crate) error: Option<String>,
    pub(crate) duration_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TextDiffSegment {
    pub(crate) kind: String,
    pub(crate) text: String,
}

#[derive(Clone, Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HistoryQuery {
    #[serde(default)]
    pub(crate) search: String,
    #[serde(default)]
    pub(crate) status: String,
    #[serde(default)]
    pub(crate) task_kind: String,
    #[serde(default)]
    pub(crate) offset: u32,
    #[serde(default = "default_page_size")]
    pub(crate) limit: u32,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HistoryPage {
    pub(crate) items: Vec<HistoryEntry>,
    pub(crate) total: u64,
    pub(crate) recovery_notice: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct NewHistoryEntry {
    pub(crate) task_kind: String,
    pub(crate) source_text: String,
    pub(crate) output_text: String,
    pub(crate) smart_processing_applied: bool,
    pub(crate) instruction: String,
    pub(crate) app_name: String,
    pub(crate) process_name: String,
    pub(crate) provider_id: String,
    pub(crate) model_id: String,
    pub(crate) status: String,
    pub(crate) error: Option<String>,
    pub(crate) duration_ms: u64,
}

#[derive(Clone, Debug, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UsageSummary {
    pub(crate) successful_actions: u64,
    pub(crate) output_chars: u64,
    pub(crate) spoken_duration_ms: u64,
    pub(crate) estimated_time_saved_ms: u64,
}

fn default_page_size() -> u32 {
    30
}

fn now_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn history_path(app: &AppHandle) -> Result<PathBuf, String> {
    crate::application::data_root::data_file(app, HISTORY_FILE)
}

pub(crate) fn open_path(path: &Path) -> Result<Connection, String> {
    let connection =
        Connection::open(path).map_err(|error| format!("打开历史数据库失败：{error}"))?;
    connection
        .execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA foreign_keys=ON;
             PRAGMA synchronous=NORMAL;
             PRAGMA busy_timeout=3000;
             CREATE TABLE IF NOT EXISTS history_entries (
               id TEXT PRIMARY KEY,
               created_at INTEGER NOT NULL,
               task_kind TEXT NOT NULL,
               source_text TEXT NOT NULL,
               output_text TEXT NOT NULL,
               final_text TEXT,
               final_text_baseline TEXT,
               final_text_confidence TEXT,
               final_text_source TEXT,
               final_text_observed_at INTEGER,
               smart_processing_applied INTEGER NOT NULL DEFAULT 0,
               learning_status TEXT NOT NULL DEFAULT 'none',
               correction_kind TEXT,
               learning_scope TEXT,
               applied_rule_ids TEXT NOT NULL DEFAULT '[]',
               instruction TEXT NOT NULL DEFAULT '',
               app_name TEXT NOT NULL DEFAULT '',
               process_name TEXT NOT NULL DEFAULT '',
               provider_id TEXT NOT NULL DEFAULT '',
               model_id TEXT NOT NULL DEFAULT '',
               status TEXT NOT NULL,
               error TEXT,
               duration_ms INTEGER NOT NULL DEFAULT 0
             );
             CREATE INDEX IF NOT EXISTS history_created_at_idx ON history_entries(created_at DESC);
             CREATE INDEX IF NOT EXISTS history_status_idx ON history_entries(status);
             CREATE TABLE IF NOT EXISTS correction_samples (
               id TEXT PRIMARY KEY,
               entry_id TEXT NOT NULL,
               before_text TEXT NOT NULL,
               after_text TEXT NOT NULL,
               app_name TEXT NOT NULL DEFAULT '',
               origin TEXT NOT NULL DEFAULT 'manual',
               confidence TEXT NOT NULL DEFAULT 'confirmed',
               capture_confidence TEXT NOT NULL DEFAULT 'confirmed',
               learning_status TEXT NOT NULL DEFAULT 'pending',
               correction_kind TEXT NOT NULL DEFAULT 'unknown',
               normalized_before TEXT NOT NULL DEFAULT '',
               normalized_after TEXT NOT NULL DEFAULT '',
               pair_key TEXT NOT NULL DEFAULT '',
               rule_key TEXT NOT NULL DEFAULT '',
               scope TEXT NOT NULL DEFAULT 'app',
               confirmed_at INTEGER,
               created_at INTEGER NOT NULL,
               FOREIGN KEY(entry_id) REFERENCES history_entries(id) ON DELETE CASCADE
             );
             CREATE TABLE IF NOT EXISTS correction_rules (
               id TEXT PRIMARY KEY,
               pair_key TEXT NOT NULL,
               rule_key TEXT NOT NULL UNIQUE,
               before_text TEXT NOT NULL,
               after_text TEXT NOT NULL,
               app_name TEXT NOT NULL DEFAULT '',
               scope TEXT NOT NULL DEFAULT 'app',
               origin TEXT NOT NULL DEFAULT 'observed',
               status TEXT NOT NULL DEFAULT 'candidate',
               evidence_count INTEGER NOT NULL DEFAULT 0,
               confirmed_count INTEGER NOT NULL DEFAULT 0,
               negative_count INTEGER NOT NULL DEFAULT 0,
               created_at INTEGER NOT NULL,
               updated_at INTEGER NOT NULL,
               last_used_at INTEGER
             );
             CREATE INDEX IF NOT EXISTS correction_rules_lookup_idx ON correction_rules(status, scope, app_name);
             CREATE TABLE IF NOT EXISTS preference_profiles (
               id TEXT PRIMARY KEY,
               scope TEXT NOT NULL,
               app_name TEXT NOT NULL DEFAULT '',
               summary_text TEXT NOT NULL,
               profile_json TEXT NOT NULL,
               status TEXT NOT NULL DEFAULT 'draft',
               sample_count INTEGER NOT NULL DEFAULT 0,
               generation_method TEXT NOT NULL DEFAULT 'llm',
               created_at INTEGER NOT NULL,
               confirmed_at INTEGER
             );
             CREATE TABLE IF NOT EXISTS history_rule_applications (
               history_id TEXT NOT NULL,
               rule_id TEXT NOT NULL,
               before_text TEXT NOT NULL,
               after_text TEXT NOT NULL,
               created_at INTEGER NOT NULL,
               negative_feedback_at INTEGER,
               PRIMARY KEY(history_id, rule_id),
               FOREIGN KEY(history_id) REFERENCES history_entries(id) ON DELETE CASCADE,
               FOREIGN KEY(rule_id) REFERENCES correction_rules(id) ON DELETE CASCADE
             );
             CREATE TABLE IF NOT EXISTS usage_totals (
               id INTEGER PRIMARY KEY CHECK (id = 1),
               successful_actions INTEGER NOT NULL DEFAULT 0,
               output_chars INTEGER NOT NULL DEFAULT 0,
               spoken_duration_ms INTEGER NOT NULL DEFAULT 0,
               estimated_time_saved_ms INTEGER NOT NULL DEFAULT 0
             );",
        )
        .map_err(|error| format!("初始化历史数据库失败：{error}"))?;
    migrate_history_schema(&connection)?;
    Ok(connection)
}

fn table_columns(
    connection: &Connection,
    table: &str,
) -> Result<std::collections::HashSet<String>, String> {
    let mut statement = connection
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(|error| format!("读取历史表结构失败：{error}"))?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|error| format!("查询历史表结构失败：{error}"))?
        .collect::<Result<std::collections::HashSet<_>, _>>()
        .map_err(|error| format!("解析历史表结构失败：{error}"))?;
    Ok(columns)
}

fn migrate_history_schema(connection: &Connection) -> Result<(), String> {
    let history = table_columns(connection, "history_entries")?;
    for (column, definition) in [
        ("final_text", "TEXT"),
        ("final_text_baseline", "TEXT"),
        ("final_text_confidence", "TEXT"),
        ("final_text_source", "TEXT"),
        ("final_text_observed_at", "INTEGER"),
        ("smart_processing_applied", "INTEGER NOT NULL DEFAULT 0"),
        ("learning_status", "TEXT NOT NULL DEFAULT 'none'"),
        ("correction_kind", "TEXT"),
        ("learning_scope", "TEXT"),
        ("applied_rule_ids", "TEXT NOT NULL DEFAULT '[]'"),
    ] {
        if !history.contains(column) {
            connection
                .execute(
                    &format!("ALTER TABLE history_entries ADD COLUMN {column} {definition}"),
                    [],
                )
                .map_err(|error| format!("升级历史字段 {column} 失败：{error}"))?;
        }
    }
    let samples = table_columns(connection, "correction_samples")?;
    for (column, definition) in [
        ("origin", "TEXT NOT NULL DEFAULT 'manual'"),
        ("confidence", "TEXT NOT NULL DEFAULT 'confirmed'"),
        ("capture_confidence", "TEXT NOT NULL DEFAULT 'confirmed'"),
        ("learning_status", "TEXT NOT NULL DEFAULT 'pending'"),
        ("correction_kind", "TEXT NOT NULL DEFAULT 'unknown'"),
        ("normalized_before", "TEXT NOT NULL DEFAULT ''"),
        ("normalized_after", "TEXT NOT NULL DEFAULT ''"),
        ("pair_key", "TEXT NOT NULL DEFAULT ''"),
        ("rule_key", "TEXT NOT NULL DEFAULT ''"),
        ("scope", "TEXT NOT NULL DEFAULT 'app'"),
        ("confirmed_at", "INTEGER"),
    ] {
        if !samples.contains(column) {
            connection
                .execute(
                    &format!("ALTER TABLE correction_samples ADD COLUMN {column} {definition}"),
                    [],
                )
                .map_err(|error| format!("升级纠错字段 {column} 失败：{error}"))?;
        }
    }
    connection
        .execute_batch(
            "CREATE INDEX IF NOT EXISTS correction_samples_pair_idx ON correction_samples(pair_key, app_name);",
        )
        .map_err(|error| format!("创建学习证据索引失败：{error}"))?;
    let applications = table_columns(connection, "history_rule_applications")?;
    if !applications.contains("negative_feedback_at") {
        connection
            .execute(
                "ALTER TABLE history_rule_applications ADD COLUMN negative_feedback_at INTEGER",
                [],
            )
            .map_err(|error| format!("升级规则反馈字段失败：{error}"))?;
    }
    Ok(())
}

fn output_metrics(text: &str, spoken_duration_ms: u64) -> (u64, u64) {
    let output_chars = text.chars().filter(|value| !value.is_whitespace()).count() as u64;
    let cjk = text
        .chars()
        .filter(|value| matches!(*value as u32, 0x3400..=0x9fff | 0xf900..=0xfaff))
        .count() as u64;
    let latin_words = text
        .split_whitespace()
        .filter(|word| word.chars().any(|value| value.is_ascii_alphabetic()))
        .count() as u64;
    let typing_ms = cjk.saturating_mul(60_000) / 40 + latin_words.saturating_mul(60_000) / 40;
    (output_chars, typing_ms.saturating_sub(spoken_duration_ms))
}

pub(crate) fn record_usage(
    app: &AppHandle,
    output: &str,
    spoken_duration_ms: u64,
) -> Result<(), String> {
    if output.trim().is_empty() {
        return Ok(());
    }
    let (output_chars, saved_ms) = output_metrics(output, spoken_duration_ms);
    open(app)?.execute(
        "INSERT INTO usage_totals (id, successful_actions, output_chars, spoken_duration_ms, estimated_time_saved_ms)
         VALUES (1, 1, ?1, ?2, ?3)
         ON CONFLICT(id) DO UPDATE SET
           successful_actions = successful_actions + 1,
           output_chars = output_chars + excluded.output_chars,
           spoken_duration_ms = spoken_duration_ms + excluded.spoken_duration_ms,
           estimated_time_saved_ms = estimated_time_saved_ms + excluded.estimated_time_saved_ms",
        params![output_chars as i64, spoken_duration_ms as i64, saved_ms as i64],
    ).map_err(|error| format!("更新本地使用统计失败：{error}"))?;
    let _ = app.emit(HISTORY_EVENT, serde_json::json!({"kind":"usageUpdated"}));
    Ok(())
}

#[tauri::command]
pub(crate) fn get_usage_summary(app: AppHandle) -> Result<UsageSummary, String> {
    open(&app)?.query_row(
        "SELECT successful_actions, output_chars, spoken_duration_ms, estimated_time_saved_ms FROM usage_totals WHERE id = 1",
        [],
        |row| Ok(UsageSummary {
            successful_actions: row.get::<_, i64>(0)?.max(0) as u64,
            output_chars: row.get::<_, i64>(1)?.max(0) as u64,
            spoken_duration_ms: row.get::<_, i64>(2)?.max(0) as u64,
            estimated_time_saved_ms: row.get::<_, i64>(3)?.max(0) as u64,
        }),
    ).optional().map(|value| value.unwrap_or_default()).map_err(|error| format!("读取本地使用统计失败：{error}"))
}

#[tauri::command]
pub(crate) fn clear_usage_summary(app: AppHandle) -> Result<(), String> {
    open(&app)?
        .execute("DELETE FROM usage_totals WHERE id = 1", [])
        .map_err(|error| format!("清空本地使用统计失败：{error}"))?;
    let _ = app.emit(HISTORY_EVENT, serde_json::json!({"kind":"usageCleared"}));
    Ok(())
}

pub(crate) fn open(app: &AppHandle) -> Result<Connection, String> {
    let path = history_path(app)?;
    match open_path(&path) {
        Ok(connection) => Ok(connection),
        Err(original_error) => recover_corrupt_database(app, &path, &original_error),
    }
}

fn recover_corrupt_database(
    app: &AppHandle,
    path: &Path,
    original_error: &str,
) -> Result<Connection, String> {
    let notice_path = crate::application::data_root::data_file(app, RECOVERY_NOTICE_FILE)?;
    recover_corrupt_path(path, &notice_path, original_error)
}

fn recover_corrupt_path(
    path: &Path,
    notice_path: &Path,
    original_error: &str,
) -> Result<Connection, String> {
    let suffix = format!("corrupt-{}", now_seconds());
    for candidate in [
        path.to_path_buf(),
        PathBuf::from(format!("{}-wal", path.display())),
        PathBuf::from(format!("{}-shm", path.display())),
    ] {
        if candidate.exists() {
            let backup = candidate.with_extension(format!(
                "{}.{}",
                candidate
                    .extension()
                    .and_then(|value| value.to_str())
                    .unwrap_or("db"),
                suffix
            ));
            std::fs::rename(&candidate, &backup).map_err(|error| {
                format!("历史数据库损坏且备份原文件失败：{original_error}；{error}")
            })?;
        }
    }
    let notice =
        format!("检测到历史数据库损坏，原文件已保留为 {suffix} 备份，并已创建新的历史数据库。");
    std::fs::write(
        notice_path,
        serde_json::to_vec(&serde_json::json!({"message": notice, "createdAt": now_seconds()}))
            .map_err(|error| format!("序列化历史恢复提示失败：{error}"))?,
    )
    .map_err(|error| format!("写入历史恢复提示失败：{error}"))?;
    open_path(path)
        .map_err(|recovery_error| format!("历史数据库恢复失败：{original_error}；{recovery_error}"))
}

fn recovery_notice(app: &AppHandle) -> Option<String> {
    let path = crate::application::data_root::data_file(app, RECOVERY_NOTICE_FILE).ok()?;
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice::<serde_json::Value>(&bytes)
        .ok()?
        .get("message")?
        .as_str()
        .map(str::to_string)
}

pub(crate) fn initialize(app: &AppHandle) -> Result<(), String> {
    let retention_days = app
        .state::<crate::state::RuntimeState>()
        .app_settings
        .lock()
        .ok()
        .and_then(|settings| {
            settings
                .history_prefs
                .get("retentionDays")
                .and_then(serde_json::Value::as_u64)
        })
        .map(|value| value.clamp(1, 3650) as u32)
        .unwrap_or(DEFAULT_RETENTION_DAYS);
    let connection = open(app)?;
    migrate_provider_ids(&connection)?;
    recover_interrupted_entries(&connection)?;
    cleanup_expired_with_connection(&connection, retention_days)?;
    crate::application::learning::migrate_existing_samples(app, &connection)?;
    let memory_days = app
        .state::<crate::state::RuntimeState>()
        .app_settings
        .lock()
        .ok()
        .and_then(|settings| {
            settings
                .history_prefs
                .get("learningMemoryRetentionDays")
                .and_then(serde_json::Value::as_u64)
        })
        .unwrap_or(180) as u32;
    crate::application::learning::cleanup_stale_rules(&connection, memory_days)?;
    crate::application::learning::refresh_cache(app, &connection)?;
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(86_400));
        interval.tick().await;
        loop {
            interval.tick().await;
            let (days, memory_days) = app
                .state::<crate::state::RuntimeState>()
                .app_settings
                .lock()
                .ok()
                .map(|settings| {
                    (
                        settings
                            .history_prefs
                            .get("retentionDays")
                            .and_then(serde_json::Value::as_u64)
                            .unwrap_or(DEFAULT_RETENTION_DAYS as u64)
                            as u32,
                        settings
                            .history_prefs
                            .get("learningMemoryRetentionDays")
                            .and_then(serde_json::Value::as_u64)
                            .unwrap_or(180) as u32,
                    )
                })
                .unwrap_or((DEFAULT_RETENTION_DAYS, 180));
            if let Err(error) = cleanup_expired(&app, days) {
                eprintln!("[history] 每日清理失败：{error}");
            }
            if let Ok(connection) = open(&app) {
                let _ = crate::application::learning::refresh_statistics(&connection);
                if crate::application::learning::cleanup_stale_rules(&connection, memory_days)
                    .is_ok()
                {
                    let _ = crate::application::learning::refresh_cache(&app, &connection);
                }
            }
        }
    });
    Ok(())
}

fn migrate_provider_ids(connection: &Connection) -> Result<(), String> {
    connection
        .execute(
            "UPDATE history_entries SET provider_id = 'bailian' WHERE provider_id = 'funasr'",
            [],
        )
        .map(|_| ())
        .map_err(|error| format!("迁移历史供应商 ID 失败：{error}"))
}

fn history_recording_allowed(
    prefs: &serde_json::Value,
    app_name: &str,
    process_name: &str,
) -> bool {
    prefs.get("enabled").and_then(serde_json::Value::as_bool) != Some(false)
        && !prefs
            .get("excludedApps")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(serde_json::Value::as_str)
            .any(|value| {
                value.eq_ignore_ascii_case(process_name.trim())
                    || value.eq_ignore_ascii_case(app_name.trim())
            })
}

pub(crate) fn record(app: &AppHandle, entry: NewHistoryEntry) -> Result<String, String> {
    record_or_update(app, None, entry)
}

/// 后处理只更新同一条记录的结果，原文与创建时间始终保留；已删除的记录不会被复活。
pub(crate) fn record_or_update(
    app: &AppHandle,
    id: Option<&str>,
    entry: NewHistoryEntry,
) -> Result<String, String> {
    if WRITES_PAUSED.load(Ordering::Acquire) {
        return Err("数据目录迁移后历史写入已暂停，请重启应用".into());
    }
    if entry.source_text.trim().is_empty() && entry.output_text.trim().is_empty() {
        return Err("空内容不会写入历史".into());
    }
    let prefs = app
        .state::<crate::state::RuntimeState>()
        .app_settings
        .lock()
        .map_err(|_| "应用配置锁失败".to_string())?
        .history_prefs
        .clone();
    if !history_recording_allowed(&prefs, &entry.app_name, &entry.process_name) {
        return Ok(String::new());
    }
    if id == Some("") {
        return Ok(String::new());
    }
    let existing_id = id;
    let id = id
        .map(str::to_owned)
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let explicit_correction = entry.task_kind == "editSelection"
        && entry.status == "succeeded"
        && !entry.source_text.trim().is_empty()
        && entry.source_text != entry.output_text;
    let correction = explicit_correction.then(|| {
        (
            entry.source_text.clone(),
            entry.output_text.clone(),
            entry.app_name.clone(),
        )
    });
    let connection = open(app)?;
    let transaction = connection
        .unchecked_transaction()
        .map_err(|error| format!("开始历史写入事务失败：{error}"))?;
    if existing_id.is_some() {
        if !update_result_with_connection(
            &transaction,
            &id,
            Some(&entry.output_text),
            &entry.status,
            entry.error.as_deref(),
            entry.duration_ms,
        )? {
            return Ok(String::new());
        }
    } else {
        insert_entry(&transaction, &id, &entry)?;
    }
    transaction
        .commit()
        .map_err(|error| format!("提交历史写入失败：{error}"))?;
    if let Some((before, after, app_name)) = correction {
        crate::application::learning::record_correction(
            app,
            &id,
            &before,
            &after,
            &app_name,
            "manual",
            "confirmed",
            None,
        )?;
    }
    if entry.status == "succeeded" {
        crate::application::final_draft::consume_pending(app, &id);
    } else if matches!(entry.status.as_str(), "failed" | "cancelled") {
        crate::application::final_draft::cancel_history(app, &id);
    }
    let kind = if existing_id.is_some() {
        "updated"
    } else {
        "created"
    };
    let _ = app.emit(HISTORY_EVENT, serde_json::json!({"kind":kind,"id":id}));
    Ok(id)
}

fn insert_entry(connection: &Connection, id: &str, entry: &NewHistoryEntry) -> Result<(), String> {
    connection
        .execute(
            "INSERT INTO history_entries
             (id, created_at, task_kind, source_text, output_text, smart_processing_applied,
              instruction, app_name, process_name, provider_id, model_id, status, error, duration_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                id,
                now_seconds(),
                entry.task_kind,
                entry.source_text,
                entry.output_text,
                entry.smart_processing_applied,
                entry.instruction,
                entry.app_name,
                entry.process_name,
                entry.provider_id,
                entry.model_id,
                entry.status,
                entry.error,
                entry.duration_ms as i64,
            ],
        )
        .map_err(|error| format!("写入历史失败：{error}"))?;
    Ok(())
}

fn update_result_with_connection(
    connection: &Connection,
    id: &str,
    output: Option<&str>,
    status: &str,
    error: Option<&str>,
    duration_ms: u64,
) -> Result<bool, String> {
    connection
        .execute(
            "UPDATE history_entries SET output_text = COALESCE(?2, output_text), status = ?3,
         error = ?4, duration_ms = ?5 WHERE id = ?1 AND status IN ('recognized', 'processed')",
            params![id, output, status, error, duration_ms as i64],
        )
        .map(|changed| changed > 0)
        .map_err(|error| format!("更新历史结果失败：{error}"))
}

pub(crate) fn update_result(
    app: &AppHandle,
    id: &str,
    output: Option<&str>,
    status: &str,
    error: Option<&str>,
    duration_ms: u64,
) -> Result<(), String> {
    if id.is_empty() {
        return Ok(());
    }
    if WRITES_PAUSED.load(Ordering::Acquire) {
        return Err("数据目录迁移后历史写入已暂停，请重启应用".into());
    }
    let connection = open(app)?;
    let identity = connection
        .query_row(
            "SELECT app_name, process_name FROM history_entries WHERE id = ?1",
            [id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()
        .map_err(|error| format!("读取历史所属应用失败：{error}"))?;
    let Some((app_name, process_name)) = identity else {
        return Ok(());
    };
    let prefs = app
        .state::<crate::state::RuntimeState>()
        .app_settings
        .lock()
        .map_err(|_| "应用配置锁失败".to_string())?
        .history_prefs
        .clone();
    if !history_recording_allowed(&prefs, &app_name, &process_name) {
        return Ok(());
    }
    if update_result_with_connection(&connection, id, output, status, error, duration_ms)? {
        if status == "succeeded" {
            crate::application::final_draft::consume_pending(app, id);
        } else if matches!(status, "failed" | "cancelled") {
            crate::application::final_draft::cancel_history(app, id);
        }
        let _ = app.emit(HISTORY_EVENT, serde_json::json!({"kind":"updated","id":id}));
    }
    Ok(())
}

fn recover_interrupted_entries(connection: &Connection) -> Result<usize, String> {
    connection.execute(
        "UPDATE history_entries SET status = 'failed', error = '上次任务未完成，已保留识别原文和现有结果'
         WHERE status IN ('recognized', 'processed')", [],
    ).map_err(|error| format!("恢复未完成历史记录失败：{error}"))
}

fn map_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<HistoryEntry> {
    let output_text: String = row.get(4)?;
    let final_text: Option<String> = row.get(5)?;
    let final_text_baseline: Option<String> = row.get(6)?;
    let diff_baseline = final_text_baseline.as_deref().unwrap_or(&output_text);
    Ok(HistoryEntry {
        id: row.get(0)?,
        created_at: row.get(1)?,
        task_kind: row.get(2)?,
        source_text: row.get(3)?,
        diff_segments: final_text
            .as_deref()
            .filter(|value| *value != diff_baseline)
            .map(|value| diff_segments(diff_baseline, value))
            .unwrap_or_default(),
        output_text,
        final_text,
        final_text_baseline,
        final_text_confidence: row.get(7)?,
        final_text_source: row.get(8)?,
        final_text_observed_at: row.get(9)?,
        smart_processing_applied: row.get::<_, i64>(10)? != 0,
        learning_status: row.get(11)?,
        correction_kind: row.get(12)?,
        learning_scope: row.get(13)?,
        applied_rule_ids: row
            .get::<_, String>(14)
            .ok()
            .and_then(|value| serde_json::from_str(&value).ok())
            .unwrap_or_default(),
        instruction: row.get(15)?,
        app_name: row.get(16)?,
        process_name: row.get(17)?,
        provider_id: row.get(18)?,
        model_id: row.get(19)?,
        status: row.get(20)?,
        error: row.get(21)?,
        duration_ms: row.get::<_, i64>(22)?.max(0) as u64,
    })
}

fn diff_segments(before: &str, after: &str) -> Vec<TextDiffSegment> {
    use similar::{ChangeTag, TextDiff};
    let mut segments: Vec<TextDiffSegment> = Vec::new();
    for change in TextDiff::from_chars(before, after).iter_all_changes() {
        let kind = match change.tag() {
            ChangeTag::Equal => "equal",
            ChangeTag::Delete => "delete",
            ChangeTag::Insert => "insert",
        };
        if let Some(last) = segments.last_mut().filter(|last| last.kind == kind) {
            last.text.push_str(change.value());
        } else {
            segments.push(TextDiffSegment {
                kind: kind.into(),
                text: change.value().into(),
            });
        }
    }
    segments
}

fn manual_correction_pair(
    output: &str,
    baseline: Option<&str>,
    final_text: &str,
) -> (String, String) {
    let Some(baseline) = baseline.filter(|baseline| *baseline != output && !output.is_empty())
    else {
        return (output.to_owned(), final_text.to_owned());
    };
    let mut matches = baseline.match_indices(output);
    let Some((start, _)) = matches.next() else {
        return (baseline.to_owned(), final_text.to_owned());
    };
    if matches.next().is_some() {
        return (baseline.to_owned(), final_text.to_owned());
    }
    let prefix = &baseline[..start];
    let suffix = &baseline[start + output.len()..];
    let Some(rest) = final_text.strip_prefix(prefix) else {
        return (baseline.to_owned(), final_text.to_owned());
    };
    let Some(after) = rest.strip_suffix(suffix) else {
        return (baseline.to_owned(), final_text.to_owned());
    };
    (output.to_owned(), after.to_owned())
}

pub(crate) fn manual_correction_pair_for_learning(
    output: &str,
    baseline: Option<&str>,
    final_text: &str,
) -> (String, String) {
    manual_correction_pair(output, baseline, final_text)
}

#[tauri::command]
pub(crate) fn query_history(app: AppHandle, query: HistoryQuery) -> Result<HistoryPage, String> {
    let connection = open(&app)?;
    let search = format!("%{}%", query.search.trim());
    let status = query.status.trim();
    let task_kind = query.task_kind.trim();
    let where_clause = "WHERE (?1 = '%%' OR source_text LIKE ?1 OR output_text LIKE ?1 OR final_text LIKE ?1 OR instruction LIKE ?1 OR app_name LIKE ?1)
                        AND (?2 = '' OR status = ?2)
                        AND (?3 = '' OR task_kind = ?3)";
    let total = connection
        .query_row(
            &format!("SELECT COUNT(*) FROM history_entries {where_clause}"),
            params![search, status, task_kind],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|error| format!("统计历史失败：{error}"))?
        .max(0) as u64;
    let limit = query.limit.clamp(1, MAX_PAGE_SIZE);
    let mut statement = connection
        .prepare(&format!(
            "SELECT id, created_at, task_kind, source_text, output_text, final_text, final_text_baseline,
                    final_text_confidence, final_text_source, final_text_observed_at,
                    smart_processing_applied, learning_status, correction_kind, learning_scope,
                    applied_rule_ids, instruction, app_name, process_name, provider_id,
                    model_id, status, error, duration_ms
             FROM history_entries {where_clause}
             ORDER BY created_at DESC LIMIT ?4 OFFSET ?5"
        ))
        .map_err(|error| format!("准备历史查询失败：{error}"))?;
    let rows = statement
        .query_map(
            params![search, status, task_kind, limit, query.offset],
            map_entry,
        )
        .map_err(|error| format!("查询历史失败：{error}"))?;
    let items = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("读取历史失败：{error}"))?;
    Ok(HistoryPage {
        items,
        total,
        recovery_notice: recovery_notice(&app),
    })
}

#[tauri::command]
pub(crate) fn update_history_text(
    app: AppHandle,
    id: String,
    output_text: String,
) -> Result<HistoryEntry, String> {
    confirm_history_final_text(app, id, output_text)
}

#[tauri::command]
pub(crate) fn confirm_history_final_text(
    app: AppHandle,
    id: String,
    final_text: String,
) -> Result<HistoryEntry, String> {
    if final_text.trim().is_empty() {
        return Err("修正后的文本不能为空".into());
    }
    let connection = open(&app)?;
    let current = get_entry(&connection, &id)?;
    if matches!(current.status.as_str(), "recognized" | "processed") {
        return Err("本次任务仍在处理，请完成后再修正；现在可以复制原文".into());
    }
    let previous: Option<(String, Option<String>, String)> = connection
        .query_row(
            "SELECT output_text, final_text_baseline, app_name FROM history_entries WHERE id = ?1",
            [&id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(|error| format!("读取待修正记录失败：{error}"))?;
    let Some((output_text, baseline, app_name)) = previous else {
        return Err("历史记录不存在".into());
    };
    let (before, correction_after) =
        manual_correction_pair(&output_text, baseline.as_deref(), &final_text);
    let transaction = connection
        .unchecked_transaction()
        .map_err(|error| format!("开始修正事务失败：{error}"))?;
    transaction
        .execute(
            "UPDATE history_entries
             SET final_text = ?1, final_text_confidence = 'confirmed',
                 final_text_source = 'manual', final_text_observed_at = ?2
             WHERE id = ?3",
            params![final_text, now_seconds(), id],
        )
        .map_err(|error| format!("更新历史文本失败：{error}"))?;
    transaction
        .commit()
        .map_err(|error| format!("提交修正失败：{error}"))?;
    if before != correction_after {
        crate::application::learning::record_correction(
            &app,
            &id,
            &before,
            &correction_after,
            &app_name,
            "manual",
            "confirmed",
            None,
        )?;
    }
    let entry = get_entry(&connection, &id)?;
    crate::application::diagnostics::event(
        "info",
        "history.finalTextConfirmed",
        serde_json::json!({
            "historyId":&id,
            "finalTextChars":final_text.chars().count(),
            "finalTextFingerprint":crate::application::diagnostics::fingerprint(&final_text),
            "status":"confirmed",
        }),
    );
    crate::application::diagnostics::content_event(
        "history.finalTextConfirmed",
        serde_json::json!({"historyId":&id,"finalText":&final_text}),
    );
    let _ = app.emit(HISTORY_EVENT, serde_json::json!({"kind":"updated","id":id}));
    Ok(entry)
}

#[tauri::command]
pub(crate) fn discard_history_final_text(
    app: AppHandle,
    id: String,
) -> Result<HistoryEntry, String> {
    let connection = open(&app)?;
    let pairs = crate::application::learning::pairs_for_history(&connection, &id)?;
    let transaction = connection
        .unchecked_transaction()
        .map_err(|error| format!("开始忽略观察结果事务失败：{error}"))?;
    let changed = transaction
        .execute(
            "UPDATE history_entries
             SET final_text = NULL, final_text_confidence = NULL, final_text_source = NULL,
                 final_text_observed_at = NULL, final_text_baseline = NULL WHERE id = ?1",
            [&id],
        )
        .map_err(|error| format!("忽略观察结果失败：{error}"))?;
    if changed == 0 {
        return Err("历史记录不存在".into());
    }
    transaction
        .execute(
            "DELETE FROM correction_samples WHERE entry_id = ?1 AND origin = 'observed'",
            [&id],
        )
        .map_err(|error| format!("删除自动纠错样本失败：{error}"))?;
    transaction
        .commit()
        .map_err(|error| format!("提交忽略观察结果失败：{error}"))?;
    crate::application::learning::recompute_pairs(&app, &connection, &pairs)?;
    let entry = get_entry(&connection, &id)?;
    crate::application::diagnostics::event(
        "info",
        "history.finalTextDiscarded",
        serde_json::json!({"historyId":&id,"status":"discarded"}),
    );
    let _ = app.emit(HISTORY_EVENT, serde_json::json!({"kind":"updated","id":id}));
    Ok(entry)
}

pub(crate) fn record_observed_final_text(
    app: &AppHandle,
    id: &str,
    final_text: &str,
    final_text_baseline: &str,
    correction_after: Option<&str>,
    confidence: &str,
    source: &str,
) -> Result<bool, String> {
    if !matches!(confidence, "high" | "medium") {
        return Err("最终草稿可信度无效".into());
    }
    if !matches!(source, "keyboard" | "click" | "autoEnter") {
        return Err("最终草稿来源无效".into());
    }
    if final_text.trim().is_empty() {
        return Err("空的最终草稿不会保存".into());
    }
    let connection = open(app)?;
    let target = connection
        .query_row(
            "SELECT status, task_kind FROM history_entries WHERE id = ?1",
            [id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()
        .map_err(|error| format!("读取最终草稿目标失败：{error}"))?;
    let Some((status, task_kind)) = target else {
        return Ok(false);
    };
    if status != "succeeded" || task_kind != "dictation" {
        return Ok(false);
    }
    let entry = get_entry(&connection, id)?;
    let transaction = connection
        .unchecked_transaction()
        .map_err(|error| format!("开始保存最终草稿事务失败：{error}"))?;
    if transaction
        .execute(
            "UPDATE history_entries SET final_text = ?1, final_text_baseline = ?2,
             final_text_confidence = ?3, final_text_source = ?4, final_text_observed_at = ?5
             WHERE id = ?6 AND status = 'succeeded' AND task_kind = 'dictation'
               AND COALESCE(final_text_confidence, '') != 'confirmed'",
            params![
                final_text,
                final_text_baseline,
                confidence,
                source,
                now_seconds(),
                id
            ],
        )
        .map_err(|error| format!("保存最终草稿失败：{error}"))?
        == 0
    {
        return Ok(false);
    }
    let correction_after = correction_after.filter(|value| !value.trim().is_empty());
    transaction
        .commit()
        .map_err(|error| format!("提交最终草稿失败：{error}"))?;
    let (learning_status, correction_kind) =
        if let Some(after) = correction_after.filter(|value| *value != entry.output_text) {
            crate::application::learning::record_correction(
                app,
                id,
                &entry.output_text,
                after,
                &entry.app_name,
                "observed",
                confidence,
                None,
            )?
        } else {
            ("none".into(), None)
        };
    crate::application::learning::record_negative_feedback(app, id, final_text)?;
    crate::application::diagnostics::event(
        "info",
        "history.observedFinalTextSaved",
        serde_json::json!({
            "historyId":id,
            "confidence":confidence,
            "source":source,
            "learningStatus":learning_status,
            "correctionKind":correction_kind,
        }),
    );
    let _ = app.emit(HISTORY_EVENT, serde_json::json!({"kind":"updated","id":id}));
    Ok(true)
}

fn get_entry(connection: &Connection, id: &str) -> Result<HistoryEntry, String> {
    connection
        .query_row(
            "SELECT id, created_at, task_kind, source_text, output_text, final_text, final_text_baseline,
                    final_text_confidence, final_text_source, final_text_observed_at,
                    smart_processing_applied, learning_status, correction_kind, learning_scope,
                    applied_rule_ids, instruction, app_name, process_name, provider_id,
                    model_id, status, error, duration_ms
             FROM history_entries WHERE id = ?1",
            [id],
            map_entry,
        )
        .optional()
        .map_err(|error| format!("读取历史记录失败：{error}"))?
        .ok_or_else(|| "历史记录不存在".to_string())
}

#[tauri::command]
pub(crate) async fn retry_history_injection(app: AppHandle, id: String) -> Result<(), String> {
    let entry = {
        let connection = open(&app)?;
        get_entry(&connection, &id)?
    };
    if matches!(entry.status.as_str(), "recognized" | "processed") {
        return Err("本次任务仍在处理，不能重复注入；现在可以复制原文".into());
    }
    let injection_text = entry
        .final_text
        .clone()
        .filter(|text| !text.trim().is_empty())
        .unwrap_or_else(|| entry.output_text.clone());
    if injection_text.trim().is_empty() {
        return Err("这条记录没有可重试注入的结果".into());
    }
    if let Some(window) = app.get_webview_window("main") {
        window
            .hide()
            .map_err(|error| format!("隐藏主窗口失败：{error}"))?;
    }
    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    let target = crate::active_app_context::activation_target()
        .ok_or_else(|| "隐藏主窗口后未找到可注入的目标窗口".to_string())?;
    let identity = crate::active_app_context::app_identity(target).unwrap_or_default();
    let matches = entry.process_name.is_empty()
        || identity
            .process_name
            .eq_ignore_ascii_case(&entry.process_name)
        || identity.app_name.eq_ignore_ascii_case(&entry.app_name);
    if !matches {
        let _ = crate::desktop::ensure_main_window(&app);
        return Err(format!(
            "当前目标是 {}，与历史记录来源 {} 不一致，已取消注入",
            identity.app_name,
            if entry.app_name.is_empty() {
                &entry.process_name
            } else {
                &entry.app_name
            }
        ));
    }
    crate::commands::dictation::inject_text_inner(injection_text, Some("paste".into())).await
}

#[tauri::command]
pub(crate) fn delete_history_entry(app: AppHandle, id: String) -> Result<(), String> {
    crate::application::final_draft::cancel_history(&app, &id);
    let connection = open(&app)?;
    let pairs = crate::application::learning::pairs_for_history(&connection, &id)?;
    let changed = connection
        .execute("DELETE FROM history_entries WHERE id = ?1", [&id])
        .map_err(|error| format!("删除历史记录失败：{error}"))?;
    if changed == 0 {
        return Err("历史记录不存在".into());
    }
    crate::application::learning::recompute_pairs(&app, &connection, &pairs)?;
    let _ = app.emit(HISTORY_EVENT, serde_json::json!({"kind":"deleted","id":id}));
    Ok(())
}

#[tauri::command]
pub(crate) fn clear_history(app: AppHandle) -> Result<(), String> {
    crate::application::final_draft::cancel_current(&app, "historyCleared");
    let connection = open(&app)?;
    connection
        .execute_batch(
            "DELETE FROM history_rule_applications;
             DELETE FROM correction_samples;
             DELETE FROM correction_rules;
             DELETE FROM preference_profiles;
             DELETE FROM history_entries;",
        )
        .map_err(|error| format!("清空历史失败：{error}"))?;
    crate::application::learning::refresh_cache(&app, &connection)?;
    let _ = app.emit(HISTORY_EVENT, serde_json::json!({"kind":"cleared"}));
    Ok(())
}

#[tauri::command]
pub(crate) fn open_history_window(app: AppHandle) -> Result<(), String> {
    crate::desktop::ensure_main_window(&app)?;
    app.emit(OPEN_HISTORY_EVENT, ())
        .map_err(|error| error.to_string())
}

pub(crate) fn cleanup_expired(app: &AppHandle, retention_days: u32) -> Result<usize, String> {
    cleanup_expired_with_connection(&open(app)?, retention_days)
}

fn cleanup_expired_with_connection(
    connection: &Connection,
    retention_days: u32,
) -> Result<usize, String> {
    let cutoff = now_seconds() - i64::from(retention_days.clamp(1, 3650)) * 86_400;
    connection
        .execute(
            "DELETE FROM history_entries WHERE created_at < ?1",
            [cutoff],
        )
        .map_err(|error| format!("清理过期历史失败：{error}"))
}

pub(crate) fn pause_for_data_root_migration(app: &AppHandle) -> Result<(), String> {
    WRITES_PAUSED.store(true, Ordering::Release);
    if !history_path(app)?.exists() {
        return Ok(());
    }
    open(app)?
        .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
        .map_err(|error| format!("迁移前整理历史数据库失败：{error}"))
}

pub(crate) fn resume_after_failed_data_root_migration() {
    WRITES_PAUSED.store(false, Ordering::Release);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_metrics_ignore_whitespace_and_never_report_negative_savings() {
        let (chars, saved) = output_metrics("你好  world", 1_000);
        assert_eq!(chars, 7);
        assert!(saved > 0);
        let (_, saved) = output_metrics("短", 120_000);
        assert_eq!(saved, 0);
    }

    #[test]
    fn recognized_text_is_durable_before_processing_and_updates_in_place() {
        let path =
            std::env::temp_dir().join(format!("sayit-history-stages-{}.sqlite3", Uuid::new_v4()));
        let connection = open_path(&path).unwrap();
        let mut raw = entry("原始识别结果");
        raw.status = "recognized".into();
        insert_entry(&connection, "one", &raw).unwrap();
        let created_at = get_entry(&connection, "one").unwrap().created_at;
        drop(connection);

        // 模拟后处理尚未完成甚至进程退出：另一个连接已能恢复原文。
        let connection = open_path(&path).unwrap();
        assert_eq!(
            get_entry(&connection, "one").unwrap().source_text,
            "原始识别结果"
        );
        assert!(update_result_with_connection(
            &connection,
            "one",
            Some("优化后的结果"),
            "processed",
            None,
            20
        )
        .unwrap());
        // 输入目标失败也不丢优化后的内容，更不覆盖 ASR 原文。
        assert!(update_result_with_connection(
            &connection,
            "one",
            None,
            "failed",
            Some("输入失败"),
            30
        )
        .unwrap());
        let saved = get_entry(&connection, "one").unwrap();
        assert_eq!(saved.source_text, "原始识别结果");
        assert_eq!(saved.output_text, "优化后的结果");
        assert_eq!(saved.created_at, created_at);
        assert_eq!(saved.status, "failed");
        assert_eq!(
            connection
                .query_row("SELECT COUNT(*) FROM history_entries", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            connection
                .query_row("SELECT COUNT(*) FROM correction_samples", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
        drop(connection);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn late_processing_cannot_overwrite_cancelled_completed_or_deleted_history() {
        let connection = Connection::open_in_memory().unwrap();
        connection.execute_batch("CREATE TABLE history_entries (id TEXT PRIMARY KEY, output_text TEXT, status TEXT, error TEXT, duration_ms INTEGER);
            INSERT INTO history_entries VALUES ('cancelled', '原文', 'cancelled', NULL, 1), ('done', '结果', 'succeeded', NULL, 1);").unwrap();
        for id in ["cancelled", "done", "deleted"] {
            assert!(!update_result_with_connection(
                &connection,
                id,
                Some("迟到结果"),
                "processed",
                None,
                2
            )
            .unwrap());
        }
    }

    #[test]
    fn restart_marks_only_unfinished_history_and_preserves_both_texts() {
        let connection = Connection::open_in_memory().unwrap();
        connection.execute_batch("CREATE TABLE history_entries (id TEXT, source_text TEXT, output_text TEXT, status TEXT, error TEXT);
            INSERT INTO history_entries VALUES ('raw', '原文', '原文', 'recognized', NULL), ('processed', '原文', '优化', 'processed', NULL), ('done', '原文', '完成', 'succeeded', NULL);").unwrap();
        assert_eq!(recover_interrupted_entries(&connection).unwrap(), 2);
        assert_eq!(recover_interrupted_entries(&connection).unwrap(), 0);
        let saved: (String, String, String) = connection.query_row("SELECT source_text, output_text, status FROM history_entries WHERE id = 'processed'", [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).unwrap();
        assert_eq!(saved, ("原文".into(), "优化".into(), "failed".into()));
    }

    #[test]
    fn staged_history_respects_recording_opt_out_and_excluded_apps() {
        assert!(!history_recording_allowed(
            &serde_json::json!({"enabled": false}),
            "Notepad",
            "notepad.exe"
        ));
        let prefs = serde_json::json!({"enabled": true, "excludedApps": ["NOTEPAD.EXE"]});
        assert!(!history_recording_allowed(&prefs, "Notepad", "notepad.exe"));
        assert!(history_recording_allowed(&prefs, "Editor", "editor.exe"));
    }

    #[test]
    fn legacy_schema_migrates_final_draft_and_sample_origin_idempotently() {
        let path = std::env::temp_dir().join(format!(
            "sayit-history-migration-{}.sqlite3",
            Uuid::new_v4()
        ));
        let legacy = Connection::open(&path).unwrap();
        legacy.execute_batch(
            "CREATE TABLE history_entries (
               id TEXT PRIMARY KEY, created_at INTEGER NOT NULL, task_kind TEXT NOT NULL,
               source_text TEXT NOT NULL, output_text TEXT NOT NULL, instruction TEXT NOT NULL DEFAULT '',
               app_name TEXT NOT NULL DEFAULT '', process_name TEXT NOT NULL DEFAULT '',
               provider_id TEXT NOT NULL DEFAULT '', model_id TEXT NOT NULL DEFAULT '',
               status TEXT NOT NULL, error TEXT, duration_ms INTEGER NOT NULL DEFAULT 0
             );
             CREATE TABLE correction_samples (
               id TEXT PRIMARY KEY, entry_id TEXT NOT NULL, before_text TEXT NOT NULL,
               after_text TEXT NOT NULL, app_name TEXT NOT NULL DEFAULT '', created_at INTEGER NOT NULL
             );
             INSERT INTO history_entries VALUES ('one', 1, 'dictation', '原文', '结果', '', '', '', '', '', 'succeeded', NULL, 0);
             INSERT INTO correction_samples VALUES ('sample', 'one', '原文', '结果', '', 1);"
        ).unwrap();
        drop(legacy);

        let connection = open_path(&path).unwrap();
        migrate_history_schema(&connection).unwrap();
        let entry = get_entry(&connection, "one").unwrap();
        assert_eq!(entry.final_text, None);
        assert!(!entry.smart_processing_applied);
        let sample: (String, String) = connection
            .query_row(
                "SELECT origin, confidence FROM correction_samples WHERE id = 'sample'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(sample, ("manual".into(), "confirmed".into()));
        drop(connection);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn final_draft_diff_uses_full_injected_baseline_without_overwriting_system_output() {
        let path =
            std::env::temp_dir().join(format!("sayit-history-final-{}.sqlite3", Uuid::new_v4()));
        let connection = open_path(&path).unwrap();
        insert_entry(&connection, "one", &entry("系统结果")).unwrap();
        connection.execute(
            "UPDATE history_entries SET final_text = '前缀系统修改后缀', final_text_baseline = '前缀系统结果后缀',
             final_text_confidence = 'high', final_text_source = 'keyboard' WHERE id = 'one'",
            [],
        ).unwrap();
        let saved = get_entry(&connection, "one").unwrap();
        assert_eq!(saved.output_text, "系统结果");
        assert_eq!(saved.final_text.as_deref(), Some("前缀系统修改后缀"));
        assert!(saved
            .diff_segments
            .iter()
            .any(|segment| segment.kind == "delete"));
        assert!(saved
            .diff_segments
            .iter()
            .any(|segment| segment.kind == "insert"));
        assert_eq!(
            manual_correction_pair("系统结果", Some("前缀系统结果后缀"), "前缀系统修改后缀",),
            ("系统结果".into(), "系统修改".into())
        );
        drop(connection);
        let _ = std::fs::remove_file(path);
    }

    fn entry(output: &str) -> NewHistoryEntry {
        NewHistoryEntry {
            task_kind: "dictation".into(),
            source_text: output.into(),
            output_text: output.into(),
            smart_processing_applied: false,
            instruction: String::new(),
            app_name: "Test".into(),
            process_name: "test.exe".into(),
            provider_id: "fake".into(),
            model_id: "fake-model".into(),
            status: "succeeded".into(),
            error: None,
            duration_ms: 12,
        }
    }

    #[test]
    fn database_roundtrip_and_correction_are_atomic() {
        let path = std::env::temp_dir().join(format!("sayit-history-{}.sqlite3", Uuid::new_v4()));
        let connection = open_path(&path).unwrap();
        let value = entry("原文");
        connection.execute(
            "INSERT INTO history_entries (id, created_at, task_kind, source_text, output_text, instruction,
             app_name, process_name, provider_id, model_id, status, error, duration_ms)
             VALUES ('one', 1, ?1, ?2, ?3, '', ?4, ?5, ?6, ?7, ?8, NULL, ?9)",
            params![value.task_kind, value.source_text, value.output_text, value.app_name,
                value.process_name, value.provider_id, value.model_id, value.status, value.duration_ms],
        ).unwrap();
        assert_eq!(get_entry(&connection, "one").unwrap().output_text, "原文");
        let removed = cleanup_expired_with_connection(&connection, 30).unwrap();
        assert_eq!(removed, 1);
        drop(connection);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn legacy_history_provider_id_migrates_idempotently() {
        let path =
            std::env::temp_dir().join(format!("sayit-history-id-{}.sqlite3", Uuid::new_v4()));
        let connection = open_path(&path).unwrap();
        connection
            .execute(
                "INSERT INTO history_entries (id, created_at, task_kind, source_text, output_text,
                 provider_id, model_id, status) VALUES ('legacy', 1, 'dictation', 'a', 'a',
                 'funasr', 'fun-asr-realtime', 'succeeded')",
                [],
            )
            .unwrap();
        migrate_provider_ids(&connection).unwrap();
        migrate_provider_ids(&connection).unwrap();
        let provider_id: String = connection
            .query_row(
                "SELECT provider_id FROM history_entries WHERE id = 'legacy'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(provider_id, "bailian");
        drop(connection);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn corrupt_database_is_preserved_before_recovery() {
        let dir = std::env::temp_dir().join(format!("sayit-history-recovery-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(HISTORY_FILE);
        let notice = dir.join(RECOVERY_NOTICE_FILE);
        std::fs::write(&path, b"not a sqlite database").unwrap();
        let error = open_path(&path).unwrap_err();
        let connection = recover_corrupt_path(&path, &notice, &error).unwrap();
        connection
            .query_row("SELECT COUNT(*) FROM history_entries", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap();
        drop(connection);
        assert!(notice.exists());
        assert!(std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .any(|entry| entry.file_name().to_string_lossy().contains("corrupt-")));
        let _ = std::fs::remove_dir_all(dir);
    }
}
