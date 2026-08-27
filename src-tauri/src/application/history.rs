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
    pub(crate) instruction: String,
    pub(crate) app_name: String,
    pub(crate) process_name: String,
    pub(crate) provider_id: String,
    pub(crate) model_id: String,
    pub(crate) status: String,
    pub(crate) error: Option<String>,
    pub(crate) duration_ms: u64,
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
    pub(crate) instruction: String,
    pub(crate) app_name: String,
    pub(crate) process_name: String,
    pub(crate) provider_id: String,
    pub(crate) model_id: String,
    pub(crate) status: String,
    pub(crate) error: Option<String>,
    pub(crate) duration_ms: u64,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CorrectionSample {
    pub(crate) before_text: String,
    pub(crate) after_text: String,
    pub(crate) app_name: String,
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

fn open_path(path: &Path) -> Result<Connection, String> {
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
               created_at INTEGER NOT NULL,
               FOREIGN KEY(entry_id) REFERENCES history_entries(id) ON DELETE CASCADE
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
    Ok(connection)
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

fn open(app: &AppHandle) -> Result<Connection, String> {
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
    cleanup_expired_with_connection(&connection, retention_days)?;
    refresh_correction_memory(app, &connection)?;
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(86_400));
        interval.tick().await;
        loop {
            interval.tick().await;
            let days = app
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
                .unwrap_or(DEFAULT_RETENTION_DAYS as u64) as u32;
            if let Err(error) = cleanup_expired(&app, days) {
                eprintln!("[history] 每日清理失败：{error}");
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

pub(crate) fn record(app: &AppHandle, entry: NewHistoryEntry) -> Result<String, String> {
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
    if prefs.get("enabled").and_then(serde_json::Value::as_bool) == Some(false) {
        return Ok(String::new());
    }
    let excluded = prefs
        .get("excludedApps")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .any(|value| {
            value.eq_ignore_ascii_case(entry.process_name.trim())
                || value.eq_ignore_ascii_case(entry.app_name.trim())
        });
    if excluded {
        return Ok(String::new());
    }
    let id = Uuid::new_v4().to_string();
    let explicit_correction = entry.task_kind == "editSelection"
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
    transaction
        .execute(
            "INSERT INTO history_entries
             (id, created_at, task_kind, source_text, output_text, instruction, app_name,
              process_name, provider_id, model_id, status, error, duration_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                id,
                now_seconds(),
                entry.task_kind,
                entry.source_text,
                entry.output_text,
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
    if let Some((before, after, app_name)) = correction {
        transaction
            .execute(
                "INSERT INTO correction_samples (id, entry_id, before_text, after_text, app_name, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![Uuid::new_v4().to_string(), id, before, after, app_name, now_seconds()],
            )
            .map_err(|error| format!("保存选区编辑纠错样本失败：{error}"))?;
    }
    transaction
        .commit()
        .map_err(|error| format!("提交历史写入失败：{error}"))?;
    if explicit_correction {
        refresh_correction_memory(app, &connection)?;
    }
    let _ = app.emit(HISTORY_EVENT, serde_json::json!({"kind":"created","id":id}));
    Ok(id)
}

fn map_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<HistoryEntry> {
    Ok(HistoryEntry {
        id: row.get(0)?,
        created_at: row.get(1)?,
        task_kind: row.get(2)?,
        source_text: row.get(3)?,
        output_text: row.get(4)?,
        instruction: row.get(5)?,
        app_name: row.get(6)?,
        process_name: row.get(7)?,
        provider_id: row.get(8)?,
        model_id: row.get(9)?,
        status: row.get(10)?,
        error: row.get(11)?,
        duration_ms: row.get::<_, i64>(12)?.max(0) as u64,
    })
}

#[tauri::command]
pub(crate) fn query_history(app: AppHandle, query: HistoryQuery) -> Result<HistoryPage, String> {
    let connection = open(&app)?;
    let search = format!("%{}%", query.search.trim());
    let status = query.status.trim();
    let task_kind = query.task_kind.trim();
    let where_clause = "WHERE (?1 = '%%' OR source_text LIKE ?1 OR output_text LIKE ?1 OR instruction LIKE ?1 OR app_name LIKE ?1)
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
            "SELECT id, created_at, task_kind, source_text, output_text, instruction, app_name,
                    process_name, provider_id, model_id, status, error, duration_ms
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
    let output_text = output_text.trim().to_string();
    if output_text.is_empty() {
        return Err("修正后的文本不能为空".into());
    }
    let connection = open(&app)?;
    let previous: Option<(String, String)> = connection
        .query_row(
            "SELECT output_text, app_name FROM history_entries WHERE id = ?1",
            [&id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|error| format!("读取待修正记录失败：{error}"))?;
    let Some((before, app_name)) = previous else {
        return Err("历史记录不存在".into());
    };
    let transaction = connection
        .unchecked_transaction()
        .map_err(|error| format!("开始修正事务失败：{error}"))?;
    transaction
        .execute(
            "UPDATE history_entries SET output_text = ?1 WHERE id = ?2",
            params![output_text, id],
        )
        .map_err(|error| format!("更新历史文本失败：{error}"))?;
    if before != output_text {
        transaction
            .execute(
                "INSERT INTO correction_samples (id, entry_id, before_text, after_text, app_name, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![Uuid::new_v4().to_string(), id, before, output_text, app_name, now_seconds()],
            )
            .map_err(|error| format!("保存纠错样本失败：{error}"))?;
    }
    transaction
        .commit()
        .map_err(|error| format!("提交修正失败：{error}"))?;
    refresh_correction_memory(&app, &connection)?;
    let entry = get_entry(&connection, &id)?;
    let _ = app.emit(HISTORY_EVENT, serde_json::json!({"kind":"updated","id":id}));
    Ok(entry)
}

fn get_entry(connection: &Connection, id: &str) -> Result<HistoryEntry, String> {
    connection
        .query_row(
            "SELECT id, created_at, task_kind, source_text, output_text, instruction, app_name,
                    process_name, provider_id, model_id, status, error, duration_ms
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
    if entry.output_text.trim().is_empty() {
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
    crate::commands::dictation::inject_text_inner(entry.output_text, Some("paste".into())).await
}

#[tauri::command]
pub(crate) fn delete_history_entry(app: AppHandle, id: String) -> Result<(), String> {
    let connection = open(&app)?;
    let changed = connection
        .execute("DELETE FROM history_entries WHERE id = ?1", [&id])
        .map_err(|error| format!("删除历史记录失败：{error}"))?;
    if changed == 0 {
        return Err("历史记录不存在".into());
    }
    refresh_correction_memory(&app, &connection)?;
    let _ = app.emit(HISTORY_EVENT, serde_json::json!({"kind":"deleted","id":id}));
    Ok(())
}

#[tauri::command]
pub(crate) fn clear_history(app: AppHandle) -> Result<(), String> {
    let connection = open(&app)?;
    connection
        .execute_batch("DELETE FROM correction_samples; DELETE FROM history_entries;")
        .map_err(|error| format!("清空历史失败：{error}"))?;
    if let Ok(mut samples) = app
        .state::<crate::state::RuntimeState>()
        .correction_samples
        .lock()
    {
        samples.clear();
    }
    let _ = app.emit(HISTORY_EVENT, serde_json::json!({"kind":"cleared"}));
    Ok(())
}

#[tauri::command]
pub(crate) fn open_history_window(app: AppHandle) -> Result<(), String> {
    crate::desktop::ensure_main_window(&app)?;
    app.emit(OPEN_HISTORY_EVENT, ())
        .map_err(|error| error.to_string())
}

fn refresh_correction_memory(app: &AppHandle, connection: &Connection) -> Result<(), String> {
    let mut statement = connection.prepare(
        "SELECT before_text, after_text, app_name FROM correction_samples ORDER BY created_at DESC LIMIT 100"
    ).map_err(|error| format!("准备纠错样本查询失败：{error}"))?;
    let samples = statement
        .query_map([], |row| {
            Ok(CorrectionSample {
                before_text: row.get(0)?,
                after_text: row.get(1)?,
                app_name: row.get(2)?,
            })
        })
        .map_err(|error| format!("查询纠错样本失败：{error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("读取纠错样本失败：{error}"))?;
    *app.state::<crate::state::RuntimeState>()
        .correction_samples
        .lock()
        .map_err(|_| "纠错样本状态锁失败")? = samples;
    Ok(())
}

pub(crate) fn relevant_corrections(
    state: &crate::state::RuntimeState,
    text: &str,
    active_context: &str,
) -> String {
    let Ok(samples) = state.correction_samples.lock() else {
        return String::new();
    };
    let mut selected = samples
        .iter()
        .filter(|sample| {
            text.contains(&sample.before_text)
                || (!sample.app_name.is_empty()
                    && active_context
                        .to_lowercase()
                        .contains(&sample.app_name.to_lowercase()))
        })
        .take(3)
        .peekable();
    if selected.peek().is_none() {
        return String::new();
    }
    let mut output = String::from("用户明确确认过的纠错示例（仅在相关时参考）：\n");
    for sample in selected {
        output.push_str(&format!(
            "- {} → {}\n",
            sample.before_text, sample.after_text
        ));
    }
    output
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

    fn entry(output: &str) -> NewHistoryEntry {
        NewHistoryEntry {
            task_kind: "dictation".into(),
            source_text: output.into(),
            output_text: output.into(),
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
