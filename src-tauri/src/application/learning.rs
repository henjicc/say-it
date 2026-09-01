use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use similar::{ChangeTag, TextDiff};
use std::collections::{BTreeMap, HashSet};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager};
use uuid::Uuid;

use crate::application::history::HISTORY_EVENT;

const SUMMARY_MIN_SAMPLES: u64 = 10;
const SUMMARY_MIN_ENTRIES: u64 = 5;
const MAX_CONTEXT_RULES: usize = 3;
const MAX_SUMMARY_SAMPLES: usize = 30;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LearningRule {
    pub(crate) id: String,
    pub(crate) pair_key: String,
    pub(crate) before_text: String,
    pub(crate) after_text: String,
    pub(crate) app_name: String,
    pub(crate) scope: String,
    pub(crate) origin: String,
    pub(crate) status: String,
    pub(crate) evidence_count: u32,
    pub(crate) confirmed_count: u32,
    pub(crate) negative_count: u32,
    pub(crate) last_used_at: Option<i64>,
    pub(crate) hotword_suggested: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PreferenceProfile {
    pub(crate) id: String,
    pub(crate) scope: String,
    pub(crate) app_name: String,
    pub(crate) summary_text: String,
    pub(crate) profile: Value,
    pub(crate) status: String,
    pub(crate) sample_count: u32,
    pub(crate) generation_method: String,
    pub(crate) created_at: i64,
    pub(crate) confirmed_at: Option<i64>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LearningOverview {
    pub(crate) observation_enabled: bool,
    pub(crate) learning_enabled: bool,
    pub(crate) cloud_context_enabled: bool,
    pub(crate) pending_count: u64,
    pub(crate) active_rule_count: u64,
    pub(crate) eligible_sample_count: u64,
    pub(crate) eligible_entry_count: u64,
    pub(crate) summary_available: bool,
    pub(crate) structured_statistics: Value,
    pub(crate) active_profile: Option<PreferenceProfile>,
    pub(crate) draft_profile: Option<PreferenceProfile>,
}

#[derive(Clone, Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LearningRuleQuery {
    #[serde(default)]
    pub(crate) search: String,
    #[serde(default)]
    pub(crate) status: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AppliedLearning {
    pub(crate) text: String,
    pub(crate) rule_ids: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LearningContext {
    pub(crate) exact_corrections: Vec<LearningContextExample>,
    pub(crate) preference_summary: Option<String>,
}

impl LearningContext {
    pub(crate) fn is_empty(&self) -> bool {
        self.exact_corrections.is_empty() && self.preference_summary.is_none()
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LearningContextExample {
    pub(crate) before: String,
    pub(crate) after: String,
    pub(crate) scope: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ReplacementCandidate {
    before: String,
    after: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Classification {
    kind: &'static str,
    candidates: Vec<ReplacementCandidate>,
}

fn now_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn hash_key(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part.as_bytes());
        hasher.update([0]);
    }
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
}

fn normalize_rule_text(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .chars()
        .map(|value| {
            if value.is_ascii() {
                value.to_ascii_lowercase()
            } else {
                value
            }
        })
        .collect()
}

fn is_format_char(value: char) -> bool {
    value.is_whitespace()
}

fn is_punctuation_char(value: char) -> bool {
    value.is_ascii_punctuation()
        || matches!(
            value as u32,
            0x2000..=0x206f | 0x3000..=0x303f | 0xff00..=0xff65
        )
}

fn ascii_token_tail(value: &str) -> &str {
    let start = value
        .char_indices()
        .rev()
        .take_while(|(_, character)| character.is_ascii_alphanumeric() || *character == '_')
        .map(|(index, _)| index)
        .last()
        .unwrap_or(value.len());
    &value[start..]
}

fn ascii_token_head(value: &str) -> &str {
    let end = value
        .char_indices()
        .take_while(|(_, character)| character.is_ascii_alphanumeric() || *character == '_')
        .map(|(index, character)| index + character.len_utf8())
        .last()
        .unwrap_or(0);
    &value[..end]
}

fn expand_single_ascii_token(before: &str, after: &str, groups: &mut [ReplacementCandidate]) {
    let [candidate] = groups else { return };
    let (Some(before_start), Some(after_start)) =
        (before.find(&candidate.before), after.find(&candidate.after))
    else {
        return;
    };
    let left_before = ascii_token_tail(&before[..before_start]);
    let left_after = ascii_token_tail(&after[..after_start]);
    let before_end = before_start + candidate.before.len();
    let after_end = after_start + candidate.after.len();
    let right_before = ascii_token_head(&before[before_end..]);
    let right_after = ascii_token_head(&after[after_end..]);
    if !left_before.is_empty() && left_before == left_after {
        candidate.before.insert_str(0, left_before);
        candidate.after.insert_str(0, left_after);
    }
    if !right_before.is_empty() && right_before == right_after {
        candidate.before.push_str(right_before);
        candidate.after.push_str(right_after);
    }
}

fn has_sensitive_content(value: &str) -> bool {
    let lowered = value.to_ascii_lowercase();
    if lowered.contains("http://")
        || lowered.contains("https://")
        || lowered.contains("www.")
        || lowered.contains("api_key")
        || lowered.contains("apikey")
        || lowered.contains("password")
        || lowered.contains("token=")
    {
        return true;
    }
    if value.split_whitespace().any(|part| {
        let at = part.find('@');
        at.is_some_and(|index| index > 0 && part[index + 1..].contains('.'))
    }) {
        return true;
    }
    let mut digit_run = 0usize;
    for value in value.chars() {
        if value.is_ascii_digit() {
            digit_run += 1;
            if digit_run >= 6 {
                return true;
            }
        } else if !matches!(value, '-' | ' ' | '(' | ')') {
            digit_run = 0;
        }
    }
    false
}

fn classify_correction(before: &str, after: &str) -> Classification {
    if before == after || before.trim().is_empty() || after.trim().is_empty() {
        return Classification {
            kind: "unknown",
            candidates: Vec::new(),
        };
    }
    if has_sensitive_content(before) || has_sensitive_content(after) {
        return Classification {
            kind: "sensitive",
            candidates: Vec::new(),
        };
    }
    let mut equal_chars = 0usize;
    let mut groups: Vec<ReplacementCandidate> = Vec::new();
    let mut deleted = String::new();
    let mut inserted = String::new();
    let flush =
        |groups: &mut Vec<ReplacementCandidate>, deleted: &mut String, inserted: &mut String| {
            if !deleted.is_empty() || !inserted.is_empty() {
                groups.push(ReplacementCandidate {
                    before: std::mem::take(deleted),
                    after: std::mem::take(inserted),
                });
            }
        };
    for change in TextDiff::from_chars(before, after).iter_all_changes() {
        match change.tag() {
            ChangeTag::Equal => {
                flush(&mut groups, &mut deleted, &mut inserted);
                equal_chars += change.value().chars().count();
            }
            ChangeTag::Delete => deleted.push_str(change.value()),
            ChangeTag::Insert => inserted.push_str(change.value()),
        }
    }
    flush(&mut groups, &mut deleted, &mut inserted);
    expand_single_ascii_token(before, after, &mut groups);
    let changed = groups
        .iter()
        .flat_map(|group| group.before.chars().chain(group.after.chars()))
        .collect::<Vec<_>>();
    if !changed.is_empty() && changed.iter().all(|value| is_format_char(*value)) {
        return Classification {
            kind: "format",
            candidates: groups,
        };
    }
    if !changed.is_empty()
        && changed
            .iter()
            .all(|value| is_format_char(*value) || is_punctuation_char(*value))
    {
        return Classification {
            kind: "punctuation",
            candidates: groups,
        };
    }
    let total = before.chars().count().max(after.chars().count()).max(1);
    let changed_max = groups
        .iter()
        .map(|group| {
            group
                .before
                .chars()
                .count()
                .max(group.after.chars().count())
        })
        .max()
        .unwrap_or(0);
    let edit_ratio = 1.0 - equal_chars as f64 / total as f64;
    let all_replacements = groups
        .iter()
        .all(|group| !group.before.is_empty() && !group.after.is_empty());
    if groups.len() <= 2
        && all_replacements
        && changed_max <= 48
        && edit_ratio <= 0.30
        && equal_chars as f64 / total as f64 >= 0.70
    {
        return Classification {
            kind: "lexical",
            candidates: groups,
        };
    }
    if groups.len() > 2 || changed_max > 80 || edit_ratio > 0.30 {
        Classification {
            kind: "rewrite",
            candidates: groups,
        }
    } else {
        Classification {
            kind: "style",
            candidates: groups,
        }
    }
}

pub(crate) fn observation_enabled(state: &crate::state::RuntimeState) -> bool {
    state.app_settings.lock().ok().and_then(|settings| {
        settings
            .history_prefs
            .get("finalDraftObservationEnabled")
            .and_then(Value::as_bool)
    }) == Some(true)
}

fn learning_enabled(state: &crate::state::RuntimeState) -> bool {
    state.app_settings.lock().ok().and_then(|settings| {
        settings
            .history_prefs
            .get("correctionLearningEnabled")
            .and_then(Value::as_bool)
    }) == Some(true)
}

fn cloud_context_enabled(state: &crate::state::RuntimeState) -> bool {
    state.app_settings.lock().ok().and_then(|settings| {
        settings
            .history_prefs
            .get("cloudLearningContextEnabled")
            .and_then(Value::as_bool)
    }) == Some(true)
}

fn provider_is_local(state: &crate::state::RuntimeState, requested_provider_id: &str) -> bool {
    let Ok(providers) = state.providers.lock() else {
        return false;
    };
    let provider_id =
        if requested_provider_id.trim().is_empty() || requested_provider_id == "default" {
            providers.defaults.llm.as_str()
        } else {
            requested_provider_id
        };
    let Some(profile) = providers
        .profiles
        .iter()
        .find(|profile| profile.id == provider_id)
    else {
        return false;
    };
    ["endpoint", "baseUrl", "apiUrl"]
        .iter()
        .filter_map(|field| profile.config.get(*field).and_then(Value::as_str))
        .any(|endpoint| {
            endpoint.contains("127.0.0.1")
                || endpoint.contains("localhost")
                || endpoint.contains("[::1]")
        })
}

fn map_rule(row: &rusqlite::Row<'_>) -> rusqlite::Result<LearningRule> {
    let after_text: String = row.get(3)?;
    let evidence_count = row.get::<_, i64>(8)?.max(0) as u32;
    Ok(LearningRule {
        id: row.get(0)?,
        pair_key: row.get(1)?,
        before_text: row.get(2)?,
        after_text: after_text.clone(),
        app_name: row.get(4)?,
        scope: row.get(5)?,
        origin: row.get(6)?,
        status: row.get(7)?,
        evidence_count,
        confirmed_count: row.get::<_, i64>(9)?.max(0) as u32,
        negative_count: row.get::<_, i64>(10)?.max(0) as u32,
        last_used_at: row.get(11)?,
        hotword_suggested: evidence_count >= 3
            && after_text.chars().count() <= 32
            && after_text.chars().any(|value| value.is_alphanumeric()),
    })
}

fn map_profile(row: &rusqlite::Row<'_>) -> rusqlite::Result<PreferenceProfile> {
    let profile_json: String = row.get(4)?;
    Ok(PreferenceProfile {
        id: row.get(0)?,
        scope: row.get(1)?,
        app_name: row.get(2)?,
        summary_text: row.get(3)?,
        profile: serde_json::from_str(&profile_json).unwrap_or_else(|_| json!({})),
        status: row.get(5)?,
        sample_count: row.get::<_, i64>(6)?.max(0) as u32,
        generation_method: row.get(7)?,
        created_at: row.get(8)?,
        confirmed_at: row.get(9)?,
    })
}

pub(crate) fn refresh_statistics(connection: &Connection) -> Result<(), String> {
    let total: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM correction_samples WHERE learning_status != 'rejected'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);
    let grouped = |column: &str| -> Result<BTreeMap<String, u64>, String> {
        let sql = format!(
            "SELECT {column}, COUNT(*) FROM correction_samples
             WHERE learning_status != 'rejected' AND {column} != '' GROUP BY {column}"
        );
        let mut statement = connection
            .prepare(&sql)
            .map_err(|error| format!("准备学习统计失败：{error}"))?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })
            .map_err(|error| format!("查询学习统计失败：{error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("读取学习统计失败：{error}"))?;
        Ok(rows
            .into_iter()
            .map(|(key, count)| (key, count.max(0) as u64))
            .collect())
    };
    let (expanded, reduced): (i64, i64) = connection
        .query_row(
            "SELECT
               SUM(CASE WHEN length(after_text) > length(before_text) THEN 1 ELSE 0 END),
               SUM(CASE WHEN length(after_text) < length(before_text) THEN 1 ELSE 0 END)
             FROM correction_samples WHERE learning_status != 'rejected'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap_or((0, 0));
    let common_replacements = {
        let mut statement = connection
            .prepare(
                "SELECT before_text, after_text, COUNT(DISTINCT entry_id) AS evidence_count
                 FROM correction_samples WHERE learning_status != 'rejected'
                   AND correction_kind = 'lexical'
                 GROUP BY pair_key ORDER BY evidence_count DESC, MAX(created_at) DESC LIMIT 20",
            )
            .map_err(|error| format!("准备常见纠错统计失败：{error}"))?;
        let rows = statement
            .query_map([], |row| {
                Ok(json!({
                    "before": row.get::<_, String>(0)?,
                    "after": row.get::<_, String>(1)?,
                    "evidenceCount": row.get::<_, i64>(2)?.max(0),
                }))
            })
            .map_err(|error| format!("查询常见纠错统计失败：{error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("读取常见纠错统计失败：{error}"))?;
        rows
    };
    let statistics = json!({
        "evidenceCount": total.max(0),
        "byKind": grouped("correction_kind")?,
        "byApplication": grouped("app_name")?,
        "expandedCount": expanded.max(0),
        "reducedCount": reduced.max(0),
        "commonReplacements": common_replacements,
        "updatedAt": now_seconds(),
    });
    connection
        .execute(
            "INSERT INTO preference_profiles
             (id, scope, app_name, summary_text, profile_json, status, sample_count,
              generation_method, created_at)
             VALUES ('statistics:global', 'global', '', '', ?1, 'statistics', ?2, 'local', ?3)
             ON CONFLICT(id) DO UPDATE SET profile_json = excluded.profile_json,
               sample_count = excluded.sample_count, created_at = excluded.created_at",
            params![statistics.to_string(), total.max(0), now_seconds()],
        )
        .map_err(|error| format!("保存本地学习统计失败：{error}"))?;
    Ok(())
}

pub(crate) fn refresh_cache(app: &AppHandle, connection: &Connection) -> Result<(), String> {
    let rules = {
        let mut statement = connection
            .prepare(
                "SELECT id, pair_key, before_text, after_text, app_name, scope, origin, status,
                        evidence_count, confirmed_count, negative_count, last_used_at
                 FROM correction_rules AS rule WHERE status = 'active'
                   AND (scope = 'global' OR NOT EXISTS (
                     SELECT 1 FROM correction_rules AS global_rule
                     WHERE global_rule.pair_key = rule.pair_key
                       AND global_rule.scope = 'global' AND global_rule.status = 'active'
                   ))
                 ORDER BY confirmed_count DESC, scope ASC, length(before_text) DESC, updated_at DESC",
            )
            .map_err(|error| format!("准备学习规则查询失败：{error}"))?;
        let rows = statement
            .query_map([], map_rule)
            .map_err(|error| format!("查询学习规则失败：{error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("读取学习规则失败：{error}"))?;
        rows
    };
    let profiles = {
        let mut statement = connection
            .prepare(
                "SELECT id, scope, app_name, summary_text, profile_json, status, sample_count,
                        generation_method, created_at, confirmed_at
                 FROM preference_profiles WHERE status = 'active' ORDER BY confirmed_at DESC",
            )
            .map_err(|error| format!("准备偏好查询失败：{error}"))?;
        let rows = statement
            .query_map([], map_profile)
            .map_err(|error| format!("查询偏好失败：{error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("读取偏好失败：{error}"))?;
        rows
    };
    *app.state::<crate::state::RuntimeState>()
        .learning_rules
        .lock()
        .map_err(|_| "学习规则状态锁失败")? = rules;
    *app.state::<crate::state::RuntimeState>()
        .preference_profiles
        .lock()
        .map_err(|_| "偏好状态锁失败")? = profiles;
    Ok(())
}

pub(crate) fn migrate_existing_samples(
    app: &AppHandle,
    connection: &Connection,
) -> Result<(), String> {
    let pending = {
        let mut statement = connection
            .prepare(
                "SELECT id, entry_id, before_text, after_text, app_name, origin, confidence
                 FROM correction_samples WHERE pair_key = ''",
            )
            .map_err(|error| format!("准备旧学习样本迁移失败：{error}"))?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                ))
            })
            .map_err(|error| format!("查询旧学习样本失败：{error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("读取旧学习样本失败：{error}"))?;
        rows
    };
    let migrated = !pending.is_empty();
    for (id, entry_id, before, after, app_name, origin, confidence) in pending {
        let classification = classify_correction(&before, &after);
        let normalized_before = normalize_rule_text(&before);
        let normalized_after = normalize_rule_text(&after);
        let pair_key = hash_key(&[&normalized_before, &normalized_after]);
        let rule_key = hash_key(&[&pair_key, "app", &app_name.to_ascii_lowercase()]);
        let active =
            origin == "manual" && confidence == "confirmed" && classification.kind != "sensitive";
        connection
            .execute(
                "UPDATE correction_samples SET capture_confidence = ?1, learning_status = ?2,
                 correction_kind = ?3, normalized_before = ?4, normalized_after = ?5,
                 pair_key = ?6, rule_key = ?7, scope = 'app', confirmed_at = ?8 WHERE id = ?9",
                params![
                    confidence,
                    if active { "active" } else { "pending" },
                    classification.kind,
                    normalized_before,
                    normalized_after,
                    pair_key,
                    rule_key,
                    active.then_some(now_seconds()),
                    id
                ],
            )
            .map_err(|error| format!("迁移旧学习样本失败：{error}"))?;
        connection
            .execute(
                "UPDATE history_entries SET learning_status = ?1, correction_kind = ?2,
                 learning_scope = 'app' WHERE id = ?3",
                params![
                    if active { "active" } else { "pending" },
                    classification.kind,
                    entry_id
                ],
            )
            .map_err(|error| format!("迁移旧历史学习状态失败：{error}"))?;
    }
    if migrated {
        rebuild_rules(connection)?;
    }
    refresh_statistics(connection)?;
    refresh_cache(app, connection)
}

fn rebuild_rules(connection: &Connection) -> Result<(), String> {
    connection
        .execute("DELETE FROM correction_rules", [])
        .map_err(|error| format!("重建学习规则失败：{error}"))?;
    let pairs = {
        let mut statement = connection
            .prepare(
                "SELECT DISTINCT pair_key, app_name FROM correction_samples
                 WHERE pair_key != '' AND correction_kind = 'lexical' AND learning_status != 'rejected'",
            )
            .map_err(|error| format!("准备规则证据查询失败：{error}"))?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|error| format!("查询规则证据失败：{error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("读取规则证据失败：{error}"))?;
        rows
    };
    for (pair_key, app_name) in pairs {
        refresh_app_rule(connection, &pair_key, &app_name, None)?;
    }
    let pair_keys = {
        let mut statement = connection
            .prepare("SELECT DISTINCT pair_key FROM correction_rules WHERE status = 'active'")
            .map_err(|error| format!("准备全局规则查询失败：{error}"))?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|error| format!("查询全局规则失败：{error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("读取全局规则失败：{error}"))?;
        rows
    };
    for pair_key in pair_keys {
        promote_global_rule(connection, &pair_key)?;
    }
    Ok(())
}

pub(crate) fn pairs_for_history(
    connection: &Connection,
    history_id: &str,
) -> Result<Vec<(String, String)>, String> {
    let mut statement = connection
        .prepare(
            "SELECT DISTINCT pair_key, app_name FROM correction_samples
             WHERE entry_id = ?1 AND pair_key != ''",
        )
        .map_err(|error| format!("准备历史学习证据查询失败：{error}"))?;
    let rows = statement
        .query_map([history_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|error| format!("查询历史学习证据失败：{error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("读取历史学习证据失败：{error}"))?;
    Ok(rows)
}

pub(crate) fn recompute_pairs(
    app: &AppHandle,
    connection: &Connection,
    pairs: &[(String, String)],
) -> Result<(), String> {
    let mut unique = HashSet::new();
    for (pair_key, app_name) in pairs {
        if unique.insert((pair_key.clone(), app_name.clone())) {
            refresh_app_rule(connection, pair_key, app_name, None)?;
            promote_global_rule(connection, pair_key)?;
        }
    }
    refresh_statistics(connection)?;
    refresh_cache(app, connection)
}

fn refresh_app_rule(
    connection: &Connection,
    pair_key: &str,
    app_name: &str,
    force_scope: Option<&str>,
) -> Result<(), String> {
    let evidence: Option<(String, String, i64, i64)> = connection
        .query_row(
            "SELECT before_text, after_text,
                    COUNT(DISTINCT CASE WHEN capture_confidence IN ('high','confirmed') THEN entry_id END),
                    SUM(CASE WHEN origin = 'manual' AND capture_confidence = 'confirmed' THEN 1 ELSE 0 END)
             FROM correction_samples
             WHERE pair_key = ?1 AND app_name = ?2 AND correction_kind = 'lexical'
               AND learning_status != 'rejected'
             GROUP BY pair_key, app_name
             ORDER BY SUM(CASE WHEN origin = 'manual' THEN 1 ELSE 0 END) DESC, MAX(created_at) DESC",
            params![pair_key, app_name],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()
        .map_err(|error| format!("统计学习证据失败：{error}"))?;
    let Some((before, after, evidence_count, confirmed_count)) = evidence else {
        connection
            .execute(
                "DELETE FROM correction_rules WHERE pair_key = ?1 AND scope = 'app' AND app_name = ?2",
                params![pair_key, app_name],
            )
            .map_err(|error| format!("删除无证据规则失败：{error}"))?;
        return Ok(());
    };
    let forced_global = force_scope == Some("global");
    let active = forced_global || confirmed_count > 0 || evidence_count >= 2;
    let scope = if forced_global { "global" } else { "app" };
    let scoped_app = if forced_global { "" } else { app_name };
    let rule_key = hash_key(&[pair_key, scope, &scoped_app.to_ascii_lowercase()]);
    let now = now_seconds();
    connection
        .execute(
            "INSERT INTO correction_rules
             (id, pair_key, rule_key, before_text, after_text, app_name, scope, origin, status,
              evidence_count, confirmed_count, negative_count, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 0, ?12, ?12)
             ON CONFLICT(rule_key) DO UPDATE SET before_text = excluded.before_text,
               after_text = excluded.after_text, origin = excluded.origin, status = excluded.status,
               evidence_count = excluded.evidence_count, confirmed_count = excluded.confirmed_count,
               updated_at = excluded.updated_at",
            params![
                Uuid::new_v4().to_string(),
                pair_key,
                rule_key,
                before,
                after,
                scoped_app,
                scope,
                if confirmed_count > 0 {
                    "manual"
                } else {
                    "observed"
                },
                if active { "active" } else { "candidate" },
                evidence_count,
                confirmed_count,
                now
            ],
        )
        .map_err(|error| format!("聚合学习规则失败：{error}"))?;
    if active {
        connection
            .execute(
                "UPDATE correction_samples SET learning_status = 'active'
                 WHERE pair_key = ?1 AND app_name = ?2 AND correction_kind = 'lexical'
                   AND learning_status = 'candidate'",
                params![pair_key, app_name],
            )
            .map_err(|error| format!("激活学习证据失败：{error}"))?;
        connection
            .execute(
                "UPDATE history_entries SET learning_status = 'active', learning_scope = ?1
                 WHERE id IN (SELECT entry_id FROM correction_samples WHERE pair_key = ?2 AND app_name = ?3)",
                params![scope, pair_key, app_name],
            )
            .map_err(|error| format!("更新历史学习状态失败：{error}"))?;
    }
    Ok(())
}

fn promote_global_rule(connection: &Connection, pair_key: &str) -> Result<(), String> {
    let explicitly_global: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM correction_samples
             WHERE pair_key = ?1 AND scope = 'global' AND learning_status != 'rejected')",
            [pair_key],
            |row| row.get(0),
        )
        .map_err(|error| format!("读取全局规则作用域失败：{error}"))?;
    let app_count: i64 = connection
        .query_row(
            "SELECT COUNT(DISTINCT app_name) FROM correction_rules
             WHERE pair_key = ?1 AND scope = 'app' AND status = 'active' AND app_name != ''",
            [pair_key],
            |row| row.get(0),
        )
        .map_err(|error| format!("统计跨应用规则失败：{error}"))?;
    if !explicitly_global && app_count < 2 {
        connection
            .execute(
                "DELETE FROM correction_rules WHERE pair_key = ?1 AND scope = 'global'",
                [pair_key],
            )
            .map_err(|error| format!("收回全局学习规则失败：{error}"))?;
        return Ok(());
    }
    let Some((before, after, evidence_count, confirmed_count)) = connection
        .query_row(
            "SELECT before_text, after_text,
                    COUNT(DISTINCT CASE WHEN capture_confidence IN ('high','confirmed') THEN entry_id END),
                    SUM(CASE WHEN origin = 'manual' AND capture_confidence = 'confirmed' THEN 1 ELSE 0 END)
             FROM correction_samples WHERE pair_key = ?1 AND correction_kind = 'lexical'
               AND learning_status != 'rejected'
             GROUP BY pair_key ORDER BY SUM(CASE WHEN origin = 'manual' THEN 1 ELSE 0 END) DESC",
            [pair_key],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)?, row.get::<_, i64>(3)?)),
        )
        .optional()
        .map_err(|error| format!("读取跨应用规则失败：{error}"))?
    else {
        return Ok(());
    };
    let rule_key = hash_key(&[pair_key, "global", ""]);
    let now = now_seconds();
    connection
        .execute(
            "INSERT INTO correction_rules
             (id, pair_key, rule_key, before_text, after_text, app_name, scope, origin, status,
              evidence_count, confirmed_count, negative_count, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, '', 'global', ?6, 'active', ?7, ?8, 0, ?9, ?9)
             ON CONFLICT(rule_key) DO UPDATE SET before_text = excluded.before_text,
               after_text = excluded.after_text, status = 'active', evidence_count = excluded.evidence_count,
               confirmed_count = excluded.confirmed_count, updated_at = excluded.updated_at",
            params![
                Uuid::new_v4().to_string(),
                pair_key,
                rule_key,
                before,
                after,
                if confirmed_count > 0 { "manual" } else { "observed" },
                evidence_count,
                confirmed_count,
                now
            ],
        )
        .map_err(|error| format!("提升全局学习规则失败：{error}"))?;
    Ok(())
}

pub(crate) fn record_correction(
    app: &AppHandle,
    entry_id: &str,
    before: &str,
    after: &str,
    app_name: &str,
    origin: &str,
    capture_confidence: &str,
    requested_scope: Option<&str>,
) -> Result<(String, Option<String>), String> {
    let classification = classify_correction(before, after);
    let state = app.state::<crate::state::RuntimeState>();
    let enabled = learning_enabled(&state);
    let connection = crate::application::history::open(app)?;
    let transaction = connection
        .unchecked_transaction()
        .map_err(|error| format!("开始学习证据事务失败：{error}"))?;
    transaction
        .execute(
            "DELETE FROM correction_samples WHERE entry_id = ?1",
            [entry_id],
        )
        .map_err(|error| format!("替换学习证据失败：{error}"))?;
    let learnable = enabled
        && classification.kind != "sensitive"
        && classification.kind != "unknown"
        && before != after;
    if !learnable {
        let status = if classification.kind == "sensitive" {
            "rejected"
        } else {
            "none"
        };
        transaction
            .execute(
                "UPDATE history_entries SET learning_status = ?1, correction_kind = ?2,
                 learning_scope = NULL WHERE id = ?3",
                params![status, classification.kind, entry_id],
            )
            .map_err(|error| format!("更新历史学习分类失败：{error}"))?;
        transaction
            .commit()
            .map_err(|error| format!("提交学习分类失败：{error}"))?;
        refresh_statistics(&connection)?;
        refresh_cache(app, &connection)?;
        return Ok((status.into(), Some(classification.kind.into())));
    }
    let confirmed = origin == "manual" && capture_confidence == "confirmed";
    let base_status = if confirmed {
        "active"
    } else if capture_confidence == "high" && classification.kind == "lexical" {
        "candidate"
    } else {
        "pending"
    };
    let scope = if requested_scope == Some("global") {
        "global"
    } else {
        "app"
    };
    let candidates = if classification.kind == "lexical" {
        classification.candidates.clone()
    } else {
        vec![ReplacementCandidate {
            before: before.to_owned(),
            after: after.to_owned(),
        }]
    };
    let now = now_seconds();
    let mut pair_apps = Vec::new();
    for candidate in candidates {
        let normalized_before = normalize_rule_text(&candidate.before);
        let normalized_after = normalize_rule_text(&candidate.after);
        if normalized_before.is_empty() || normalized_after.is_empty() {
            continue;
        }
        let pair_key = hash_key(&[&normalized_before, &normalized_after]);
        let rule_key = hash_key(&[&pair_key, scope, &app_name.to_ascii_lowercase()]);
        transaction
            .execute(
                "INSERT INTO correction_samples
                 (id, entry_id, before_text, after_text, app_name, origin, confidence,
                  capture_confidence, learning_status, correction_kind, normalized_before,
                  normalized_after, pair_key, rule_key, scope, confirmed_at, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
                params![
                    Uuid::new_v4().to_string(),
                    entry_id,
                    candidate.before,
                    candidate.after,
                    app_name,
                    origin,
                    capture_confidence,
                    base_status,
                    classification.kind,
                    normalized_before,
                    normalized_after,
                    pair_key,
                    rule_key,
                    scope,
                    confirmed.then_some(now),
                    now
                ],
            )
            .map_err(|error| format!("保存学习证据失败：{error}"))?;
        pair_apps.push((pair_key, app_name.to_owned()));
    }
    transaction
        .execute(
            "UPDATE history_entries SET learning_status = ?1, correction_kind = ?2,
             learning_scope = ?3 WHERE id = ?4",
            params![base_status, classification.kind, scope, entry_id],
        )
        .map_err(|error| format!("更新历史学习状态失败：{error}"))?;
    transaction
        .commit()
        .map_err(|error| format!("提交学习证据失败：{error}"))?;
    refresh_statistics(&connection)?;
    if classification.kind == "lexical" {
        let mut seen = HashSet::new();
        for (pair_key, app_name) in pair_apps {
            if seen.insert((pair_key.clone(), app_name.clone())) {
                refresh_app_rule(&connection, &pair_key, &app_name, requested_scope)?;
                promote_global_rule(&connection, &pair_key)?;
            }
        }
    }
    refresh_cache(app, &connection)?;
    let status: String = connection
        .query_row(
            "SELECT learning_status FROM history_entries WHERE id = ?1",
            [entry_id],
            |row| row.get(0),
        )
        .unwrap_or_else(|_| base_status.into());
    crate::application::diagnostics::event(
        "info",
        "learning.evidenceRecorded",
        json!({
            "historyId":entry_id,
            "captureConfidence":capture_confidence,
            "correctionKind":classification.kind,
            "learningStatus":status,
            "origin":origin,
        }),
    );
    Ok((status, Some(classification.kind.into())))
}

fn match_is_protected(text: &str, start: usize, end: usize, pattern: &str) -> bool {
    if text[..start]
        .chars()
        .filter(|character| *character == '`')
        .count()
        % 2
        == 1
    {
        return true;
    }
    let bytes = text.as_bytes();
    let token_start = bytes[..start]
        .iter()
        .rposition(|value| value.is_ascii_whitespace())
        .map(|index| index + 1)
        .unwrap_or(0);
    let token_end = bytes[end..]
        .iter()
        .position(|value| value.is_ascii_whitespace())
        .map(|index| end + index)
        .unwrap_or(text.len());
    let token = text.get(token_start..token_end).unwrap_or_default();
    if has_sensitive_content(token) || token.contains('`') {
        return true;
    }
    if pattern
        .chars()
        .next()
        .is_some_and(|value| value.is_ascii_alphanumeric())
    {
        let before = text[..start].chars().next_back();
        let after = text[end..].chars().next();
        if before.is_some_and(|value| value.is_ascii_alphanumeric() || value == '_')
            || after.is_some_and(|value| value.is_ascii_alphanumeric() || value == '_')
        {
            return true;
        }
    }
    false
}

fn replace_once_safe(text: &str, before: &str, after: &str) -> Option<String> {
    for (start, _) in text.match_indices(before) {
        let end = start + before.len();
        if match_is_protected(text, start, end, before) {
            continue;
        }
        let mut output = String::with_capacity(text.len() + after.len());
        output.push_str(&text[..start]);
        output.push_str(after);
        output.push_str(&text[end..]);
        return Some(output);
    }
    None
}

pub(crate) fn apply_active_rules(
    state: &crate::state::RuntimeState,
    text: &str,
    app_name: &str,
) -> AppliedLearning {
    if !learning_enabled(state) || text.is_empty() {
        return AppliedLearning {
            text: text.to_owned(),
            rule_ids: Vec::new(),
        };
    }
    let Ok(cached) = state.learning_rules.lock() else {
        return AppliedLearning {
            text: text.to_owned(),
            rule_ids: Vec::new(),
        };
    };
    let mut rules = cached
        .iter()
        .filter(|rule| {
            rule.status == "active"
                && (rule.scope == "global" || rule.app_name.eq_ignore_ascii_case(app_name))
                && text.contains(&rule.before_text)
        })
        .cloned()
        .collect::<Vec<_>>();
    rules.sort_by(|left, right| {
        right
            .confirmed_count
            .cmp(&left.confirmed_count)
            .then_with(|| (left.scope == "global").cmp(&(right.scope == "global")))
            .then_with(|| {
                right
                    .before_text
                    .chars()
                    .count()
                    .cmp(&left.before_text.chars().count())
            })
    });
    drop(cached);
    let mut output = text.to_owned();
    let mut seen_outputs = HashSet::from([output.clone()]);
    let mut rule_ids = Vec::new();
    for rule in rules {
        if let Some(next) = replace_once_safe(&output, &rule.before_text, &rule.after_text) {
            if !seen_outputs.insert(next.clone()) {
                continue;
            }
            output = next;
            rule_ids.push(rule.id);
        }
    }
    AppliedLearning {
        text: output,
        rule_ids,
    }
}

pub(crate) fn record_rule_applications(
    app: &AppHandle,
    history_id: &str,
    rule_ids: &[String],
) -> Result<(), String> {
    if history_id.is_empty() || rule_ids.is_empty() {
        return Ok(());
    }
    let connection = crate::application::history::open(app)?;
    let transaction = connection
        .unchecked_transaction()
        .map_err(|error| format!("开始记录学习规则应用失败：{error}"))?;
    let now = now_seconds();
    for rule_id in rule_ids {
        let rule = transaction
            .query_row(
                "SELECT before_text, after_text FROM correction_rules WHERE id = ?1 AND status = 'active'",
                [rule_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()
            .map_err(|error| format!("读取已应用规则失败：{error}"))?;
        if let Some((before, after)) = rule {
            transaction
                .execute(
                    "INSERT OR REPLACE INTO history_rule_applications
                     (history_id, rule_id, before_text, after_text, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![history_id, rule_id, before, after, now],
                )
                .map_err(|error| format!("保存规则应用失败：{error}"))?;
            transaction
                .execute(
                    "UPDATE correction_rules SET last_used_at = ?1, updated_at = ?1 WHERE id = ?2",
                    params![now, rule_id],
                )
                .map_err(|error| format!("更新规则使用时间失败：{error}"))?;
        }
    }
    let encoded = serde_json::to_string(rule_ids).map_err(|error| error.to_string())?;
    transaction
        .execute(
            "UPDATE history_entries SET applied_rule_ids = ?1 WHERE id = ?2",
            params![encoded, history_id],
        )
        .map_err(|error| format!("更新历史规则应用失败：{error}"))?;
    transaction
        .commit()
        .map_err(|error| format!("提交规则应用失败：{error}"))?;
    refresh_cache(app, &connection)
}

pub(crate) fn record_negative_feedback(
    app: &AppHandle,
    history_id: &str,
    final_text: &str,
) -> Result<(), String> {
    let connection = crate::application::history::open(app)?;
    if record_negative_feedback_with_connection(&connection, history_id, final_text)? {
        refresh_cache(app, &connection)?;
    }
    Ok(())
}

fn record_negative_feedback_with_connection(
    connection: &Connection,
    history_id: &str,
    final_text: &str,
) -> Result<bool, String> {
    let applications = {
        let mut statement = connection
            .prepare(
                "SELECT rule_id, before_text, after_text FROM history_rule_applications
                 WHERE history_id = ?1 AND negative_feedback_at IS NULL",
            )
            .map_err(|error| format!("准备规则反馈查询失败：{error}"))?;
        let rows = statement
            .query_map([history_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(|error| format!("查询规则反馈失败：{error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("读取规则反馈失败：{error}"))?;
        rows
    };
    let mut changed = false;
    for (rule_id, before, after) in applications {
        if final_text.contains(&before) && !final_text.contains(&after) {
            let transaction = connection
                .unchecked_transaction()
                .map_err(|error| format!("开始记录规则负反馈失败：{error}"))?;
            let recorded = transaction
                .execute(
                    "UPDATE history_rule_applications SET negative_feedback_at = ?1
                     WHERE history_id = ?2 AND rule_id = ?3 AND negative_feedback_at IS NULL",
                    params![now_seconds(), history_id, rule_id],
                )
                .map_err(|error| format!("标记规则负反馈失败：{error}"))?;
            if recorded == 0 {
                continue;
            }
            transaction
                .execute(
                    "UPDATE correction_rules SET negative_count = negative_count + 1,
                     status = CASE WHEN negative_count + 1 >= 2 THEN 'disabled' ELSE status END,
                     updated_at = ?1 WHERE id = ?2",
                    params![now_seconds(), rule_id],
                )
                .map_err(|error| format!("记录规则负反馈失败：{error}"))?;
            transaction
                .commit()
                .map_err(|error| format!("提交规则负反馈失败：{error}"))?;
            changed = true;
        }
    }
    Ok(changed)
}

pub(crate) fn build_context(
    state: &crate::state::RuntimeState,
    text: &str,
    app_name: &str,
    provider_id: &str,
) -> LearningContext {
    if !learning_enabled(state)
        || (!provider_is_local(state, provider_id) && !cloud_context_enabled(state))
    {
        return LearningContext::default();
    }
    let rules = state
        .learning_rules
        .lock()
        .map(|rules| {
            rules
                .iter()
                .filter(|rule| {
                    rule.status == "active"
                        && text.contains(&rule.before_text)
                        && (rule.scope == "global" || rule.app_name.eq_ignore_ascii_case(app_name))
                        && !has_sensitive_content(&rule.before_text)
                        && !has_sensitive_content(&rule.after_text)
                })
                .take(MAX_CONTEXT_RULES)
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let profile = state.preference_profiles.lock().ok().and_then(|profiles| {
        profiles
            .iter()
            .find(|profile| {
                profile.status == "active"
                    && (profile.scope == "global"
                        || profile.app_name.eq_ignore_ascii_case(app_name))
            })
            .cloned()
    });
    LearningContext {
        exact_corrections: rules
            .into_iter()
            .map(|rule| LearningContextExample {
                before: rule.before_text,
                after: rule.after_text,
                scope: rule.scope,
            })
            .collect(),
        preference_summary: profile.map(|profile| profile.summary_text),
    }
}

#[tauri::command]
pub(crate) fn get_learning_overview(app: AppHandle) -> Result<LearningOverview, String> {
    let connection = crate::application::history::open(&app)?;
    let (observation_enabled, learning_enabled, cloud_context_enabled) = app
        .state::<crate::state::RuntimeState>()
        .app_settings
        .lock()
        .map(|settings| {
            (
                settings
                    .history_prefs
                    .get("finalDraftObservationEnabled")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                settings
                    .history_prefs
                    .get("correctionLearningEnabled")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                settings
                    .history_prefs
                    .get("cloudLearningContextEnabled")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            )
        })
        .unwrap_or((false, false, false));
    let pending_count = connection
        .query_row(
            "SELECT COUNT(DISTINCT entry_id) FROM correction_samples WHERE learning_status IN ('pending','candidate')",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0)
        .max(0) as u64;
    let active_rule_count = connection
        .query_row(
            "SELECT COUNT(*) FROM correction_rules AS rule WHERE status = 'active'
             AND (scope = 'global' OR NOT EXISTS (
               SELECT 1 FROM correction_rules AS global_rule
               WHERE global_rule.pair_key = rule.pair_key
                 AND global_rule.scope = 'global' AND global_rule.status = 'active'
             ))",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0)
        .max(0) as u64;
    let (eligible_sample_count, eligible_entry_count) = connection
        .query_row(
            "SELECT COUNT(*), COUNT(DISTINCT entry_id) FROM correction_samples
             WHERE learning_status = 'active' AND correction_kind != 'sensitive'",
            [],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        )
        .unwrap_or((0, 0));
    let profiles = {
        let mut statement = connection
            .prepare(
                "SELECT id, scope, app_name, summary_text, profile_json, status, sample_count,
                        generation_method, created_at, confirmed_at
                 FROM preference_profiles WHERE status IN ('active','draft') ORDER BY created_at DESC",
            )
            .map_err(|error| format!("准备偏好概览失败：{error}"))?;
        let rows = statement
            .query_map([], map_profile)
            .map_err(|error| format!("查询偏好概览失败：{error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("读取偏好概览失败：{error}"))?;
        rows
    };
    let structured_statistics = connection
        .query_row(
            "SELECT profile_json FROM preference_profiles WHERE id = 'statistics:global'",
            [],
            |row| row.get::<_, String>(0),
        )
        .ok()
        .and_then(|value| serde_json::from_str(&value).ok())
        .unwrap_or_else(|| json!({}));
    Ok(LearningOverview {
        observation_enabled,
        learning_enabled,
        cloud_context_enabled,
        pending_count,
        active_rule_count,
        eligible_sample_count: eligible_sample_count.max(0) as u64,
        eligible_entry_count: eligible_entry_count.max(0) as u64,
        summary_available: eligible_sample_count.max(0) as u64 >= SUMMARY_MIN_SAMPLES
            && eligible_entry_count.max(0) as u64 >= SUMMARY_MIN_ENTRIES,
        structured_statistics,
        active_profile: profiles
            .iter()
            .find(|profile| profile.status == "active")
            .cloned(),
        draft_profile: profiles
            .iter()
            .find(|profile| profile.status == "draft")
            .cloned(),
    })
}

#[tauri::command]
pub(crate) fn query_learning_rules(
    app: AppHandle,
    query: Option<LearningRuleQuery>,
) -> Result<Vec<LearningRule>, String> {
    let query = query.unwrap_or_default();
    let connection = crate::application::history::open(&app)?;
    let search = format!("%{}%", query.search.trim());
    let mut statement = connection
        .prepare(
            "SELECT id, pair_key, before_text, after_text, app_name, scope, origin, status,
                    evidence_count, confirmed_count, negative_count, last_used_at
             FROM correction_rules
             WHERE (?1 = '%%' OR before_text LIKE ?1 OR after_text LIKE ?1 OR app_name LIKE ?1)
               AND (?2 = '' OR status = ?2)
               AND (scope = 'global' OR NOT EXISTS (
                 SELECT 1 FROM correction_rules AS global_rule
                 WHERE global_rule.pair_key = correction_rules.pair_key
                   AND global_rule.scope = 'global' AND global_rule.status IN ('active','disabled')
               ))
             ORDER BY CASE status WHEN 'active' THEN 0 WHEN 'candidate' THEN 1 ELSE 2 END,
                      confirmed_count DESC, updated_at DESC LIMIT 200",
        )
        .map_err(|error| format!("准备学习规则列表失败：{error}"))?;
    let rows = statement
        .query_map(params![search, query.status.trim()], map_rule)
        .map_err(|error| format!("查询学习规则列表失败：{error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("读取学习规则列表失败：{error}"))?;
    Ok(rows)
}

#[tauri::command]
pub(crate) fn confirm_history_learning(
    app: AppHandle,
    id: String,
    scope: Option<String>,
) -> Result<(), String> {
    let connection = crate::application::history::open(&app)?;
    let entry = connection
        .query_row(
            "SELECT output_text, final_text, final_text_baseline, app_name FROM history_entries WHERE id = ?1",
            [&id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .optional()
        .map_err(|error| format!("读取待确认学习记录失败：{error}"))?
        .ok_or("历史记录不存在")?;
    let final_text = entry
        .1
        .filter(|value| !value.trim().is_empty())
        .ok_or("没有可确认的最终草稿")?;
    let (before, after) = crate::application::history::manual_correction_pair_for_learning(
        &entry.0,
        entry.2.as_deref(),
        &final_text,
    );
    record_correction(
        &app,
        &id,
        &before,
        &after,
        &entry.3,
        "manual",
        "confirmed",
        scope.as_deref(),
    )?;
    let _ = app.emit(HISTORY_EVENT, json!({"kind":"updated","id":id}));
    Ok(())
}

#[tauri::command]
pub(crate) fn reject_history_learning(app: AppHandle, id: String) -> Result<(), String> {
    let connection = crate::application::history::open(&app)?;
    let pairs = pairs_for_history(&connection, &id)?;
    connection
        .execute(
            "UPDATE correction_samples SET learning_status = 'rejected' WHERE entry_id = ?1",
            [&id],
        )
        .map_err(|error| format!("拒绝学习证据失败：{error}"))?;
    connection
        .execute(
            "UPDATE history_entries SET learning_status = 'rejected' WHERE id = ?1",
            [&id],
        )
        .map_err(|error| format!("更新历史学习状态失败：{error}"))?;
    recompute_pairs(&app, &connection, &pairs)?;
    let _ = app.emit(HISTORY_EVENT, json!({"kind":"updated","id":id}));
    Ok(())
}

#[tauri::command]
pub(crate) fn set_learning_rule_scope(
    app: AppHandle,
    id: String,
    scope: String,
) -> Result<(), String> {
    if !matches!(scope.as_str(), "app" | "global") {
        return Err("学习规则作用域无效".into());
    }
    let connection = crate::application::history::open(&app)?;
    let rule = connection
        .query_row(
            "SELECT pair_key, app_name, status FROM correction_rules WHERE id = ?1",
            [&id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()
        .map_err(|error| format!("读取学习规则失败：{error}"))?
        .ok_or("学习规则不存在")?;
    if rule.2 != "active" {
        return Err("只有已学习规则可以修改作用域".into());
    }
    if scope == "global" {
        connection
            .execute(
                "UPDATE correction_samples SET scope = 'global' WHERE pair_key = ?1 AND learning_status != 'rejected'",
                [&rule.0],
            )
            .map_err(|error| format!("保存全局规则作用域失败：{error}"))?;
        promote_global_rule(&connection, &rule.0)?;
    } else {
        connection
            .execute(
                "DELETE FROM correction_rules WHERE pair_key = ?1 AND scope = 'global'",
                [&rule.0],
            )
            .map_err(|error| format!("恢复应用内规则失败：{error}"))?;
        let app_name = if rule.1.is_empty() {
            connection
                .query_row(
                    "SELECT app_name FROM correction_samples WHERE pair_key = ?1 AND app_name != ''
                     ORDER BY confirmed_at DESC, created_at DESC LIMIT 1",
                    [&rule.0],
                    |row| row.get::<_, String>(0),
                )
                .optional()
                .map_err(|error| format!("恢复应用内规则失败：{error}"))?
                .ok_or("该规则没有可恢复的应用来源")?
        } else {
            rule.1.clone()
        };
        connection
            .execute(
                "UPDATE correction_samples SET learning_status = CASE WHEN app_name = ?1 THEN learning_status ELSE 'rejected' END,
                 scope = 'app' WHERE pair_key = ?2",
                params![app_name, rule.0],
            )
            .map_err(|error| format!("保存应用内规则作用域失败：{error}"))?;
        refresh_app_rule(&connection, &rule.0, &app_name, None)?;
        promote_global_rule(&connection, &rule.0)?;
    }
    refresh_cache(&app, &connection)
}

#[tauri::command]
pub(crate) fn delete_learning_rule(app: AppHandle, id: String) -> Result<(), String> {
    let connection = crate::application::history::open(&app)?;
    let target = connection
        .query_row(
            "SELECT pair_key, app_name, scope FROM correction_rules WHERE id = ?1",
            [&id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()
        .map_err(|error| format!("读取待删除规则失败：{error}"))?
        .ok_or("学习规则不存在")?;
    if target.2 == "global" {
        connection
            .execute(
                "UPDATE correction_samples SET learning_status = 'rejected' WHERE pair_key = ?1",
                [&target.0],
            )
            .map_err(|error| format!("拒绝全局规则证据失败：{error}"))?;
        connection
            .execute(
                "DELETE FROM correction_rules WHERE pair_key = ?1",
                [&target.0],
            )
            .map_err(|error| format!("删除全局学习规则失败：{error}"))?;
    } else {
        connection
            .execute(
                "UPDATE correction_samples SET learning_status = 'rejected' WHERE pair_key = ?1 AND app_name = ?2",
                params![target.0, target.1],
            )
            .map_err(|error| format!("拒绝应用规则证据失败：{error}"))?;
        refresh_app_rule(&connection, &target.0, &target.1, None)?;
        promote_global_rule(&connection, &target.0)?;
    }
    refresh_statistics(&connection)?;
    refresh_cache(&app, &connection)
}

#[tauri::command]
pub(crate) fn set_learning_rule_enabled(
    app: AppHandle,
    id: String,
    enabled: bool,
) -> Result<(), String> {
    let connection = crate::application::history::open(&app)?;
    let target = connection
        .query_row(
            "SELECT pair_key, scope, status FROM correction_rules WHERE id = ?1",
            [&id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()
        .map_err(|error| format!("读取学习规则失败：{error}"))?
        .ok_or("学习规则不存在")?;
    if enabled && target.2 != "disabled" {
        return Err("只有已停用规则可以重新启用".into());
    }
    if !enabled && target.2 != "active" {
        return Err("只有生效中的规则可以停用".into());
    }
    if target.1 == "global" {
        connection
            .execute(
                "UPDATE correction_rules SET status = ?1, updated_at = ?2
                 WHERE pair_key = ?3 AND status = ?4",
                params![
                    if enabled { "active" } else { "disabled" },
                    now_seconds(),
                    target.0,
                    if enabled { "disabled" } else { "active" }
                ],
            )
            .map_err(|error| format!("更新全局学习规则状态失败：{error}"))?;
    } else {
        connection
            .execute(
                "UPDATE correction_rules SET status = ?1, updated_at = ?2 WHERE id = ?3",
                params![
                    if enabled { "active" } else { "disabled" },
                    now_seconds(),
                    id
                ],
            )
            .map_err(|error| format!("更新学习规则状态失败：{error}"))?;
    }
    crate::application::diagnostics::event(
        "info",
        if enabled {
            "learning.ruleEnabled"
        } else {
            "learning.ruleDisabled"
        },
        json!({"ruleId":id,"scope":target.1}),
    );
    refresh_cache(&app, &connection)
}

fn extract_json(value: &str) -> Result<Value, String> {
    let trimmed = value.trim();
    let candidate = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .and_then(|value| value.strip_suffix("```"))
        .map(str::trim)
        .unwrap_or(trimmed);
    serde_json::from_str(candidate).map_err(|error| format!("偏好总结返回格式无效：{error}"))
}

fn validate_preference_profile(value: &Value, sample_count: usize) -> Result<String, String> {
    let object = value.as_object().ok_or("偏好总结必须是 JSON 对象")?;
    if object
        .keys()
        .any(|key| !matches!(key.as_str(), "summary" | "preferences" | "avoid"))
    {
        return Err("偏好总结包含未知字段".into());
    }
    let summary = object
        .get("summary")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty() && value.chars().count() <= 500)
        .ok_or("偏好总结缺少有效 summary")?;
    let preferences = object
        .get("preferences")
        .and_then(Value::as_array)
        .ok_or("偏好总结缺少 preferences 数组")?;
    if preferences.len() > 20 {
        return Err("偏好总结包含过多偏好项".into());
    }
    for preference in preferences {
        let item = preference.as_object().ok_or("偏好项必须是对象")?;
        let instruction = item
            .get("instruction")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty() && value.chars().count() <= 300)
            .ok_or("偏好项缺少有效 instruction")?;
        let _ = instruction;
        let evidence_count = item
            .get("evidenceCount")
            .and_then(Value::as_u64)
            .ok_or("偏好项缺少 evidenceCount")?;
        if evidence_count == 0 || evidence_count > sample_count as u64 {
            return Err("偏好项 evidenceCount 超出本次证据范围".into());
        }
        if !matches!(
            item.get("confidence").and_then(Value::as_str),
            Some("high" | "medium")
        ) {
            return Err("偏好项 confidence 无效".into());
        }
    }
    let avoid = object
        .get("avoid")
        .and_then(Value::as_array)
        .ok_or("偏好总结缺少 avoid 数组")?;
    if avoid.len() > 20
        || avoid.iter().any(|item| {
            item.as_str()
                .is_none_or(|value| value.trim().is_empty() || value.trim().chars().count() > 300)
        })
    {
        return Err("偏好总结的 avoid 内容无效".into());
    }
    Ok(summary.to_owned())
}

#[tauri::command]
pub(crate) async fn generate_preference_summary(
    app: AppHandle,
    scope: String,
    provider_id: String,
    allow_cloud: bool,
) -> Result<PreferenceProfile, String> {
    if scope != "global" && !scope.starts_with("app:") {
        return Err("偏好总结作用域无效".into());
    }
    let state = app.state::<crate::state::RuntimeState>();
    if !provider_is_local(&state, &provider_id) && !allow_cloud {
        return Err("当前模型需要联网；请明确确认本次允许发送脱敏的局部学习样本".into());
    }
    let connection = crate::application::history::open(&app)?;
    let app_name = scope.strip_prefix("app:").unwrap_or("");
    let (sample_count, entry_count): (i64, i64) = connection
        .query_row(
            "SELECT COUNT(*), COUNT(DISTINCT entry_id) FROM correction_samples
             WHERE learning_status = 'active' AND correction_kind != 'sensitive'
               AND (?1 = '' OR app_name = ?1)",
            [app_name],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|error| format!("统计偏好总结样本失败：{error}"))?;
    if sample_count < SUMMARY_MIN_SAMPLES as i64 || entry_count < SUMMARY_MIN_ENTRIES as i64 {
        return Err("至少需要 10 条有效证据且来自 5 次不同听写，才能生成表达偏好".into());
    }
    let samples = {
        let mut statement = connection
            .prepare(
                "SELECT before_text, after_text, app_name, correction_kind FROM correction_samples
                 WHERE learning_status = 'active' AND correction_kind != 'sensitive'
                   AND (?1 = '' OR app_name = ?1)
                 ORDER BY CASE origin WHEN 'manual' THEN 0 ELSE 1 END, created_at DESC LIMIT ?2",
            )
            .map_err(|error| format!("准备偏好总结样本失败：{error}"))?;
        let rows = statement
            .query_map(params![app_name, MAX_SUMMARY_SAMPLES as i64], |row| {
                Ok(json!({
                    "before": row.get::<_, String>(0)?,
                    "after": row.get::<_, String>(1)?,
                    "application": row.get::<_, String>(2)?,
                    "kind": row.get::<_, String>(3)?,
                }))
            })
            .map_err(|error| format!("查询偏好总结样本失败：{error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("读取偏好总结样本失败：{error}"))?;
        rows
    };
    let prompt = format!(
        "请仅根据以下已确认的局部修改，总结稳定、可复用的表达偏好。不要推断身份、职业或敏感属性。\n\
         返回严格 JSON：{{\"summary\":\"一句简洁偏好说明\",\"preferences\":[{{\"instruction\":\"偏好\",\"evidenceCount\":1,\"confidence\":\"high|medium\"}}],\"avoid\":[\"应避免的行为\"]}}。\n\
         样本：{}",
        serde_json::to_string(&samples).map_err(|error| error.to_string())?
    );
    let output = crate::application::smart_text::process_prompt(
        &state,
        "你负责从用户明确确认的文本修改中提炼保守、可验证的写作偏好。",
        &prompt,
        Some(&provider_id),
        None,
        "learning-summary",
        true,
    )
    .await?;
    let profile_json = extract_json(&output)?;
    let summary_text = validate_preference_profile(&profile_json, samples.len())?;
    let id = Uuid::new_v4().to_string();
    let created_at = now_seconds();
    connection
        .execute(
            "INSERT INTO preference_profiles
             (id, scope, app_name, summary_text, profile_json, status, sample_count,
              generation_method, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 'draft', ?6, 'llm', ?7)",
            params![
                id,
                if app_name.is_empty() { "global" } else { "app" },
                app_name,
                summary_text,
                profile_json.to_string(),
                samples.len() as i64,
                created_at
            ],
        )
        .map_err(|error| format!("保存偏好总结草稿失败：{error}"))?;
    let profile = PreferenceProfile {
        id,
        scope: if app_name.is_empty() { "global" } else { "app" }.into(),
        app_name: app_name.into(),
        summary_text,
        profile: profile_json,
        status: "draft".into(),
        sample_count: samples.len() as u32,
        generation_method: "llm".into(),
        created_at,
        confirmed_at: None,
    };
    crate::application::diagnostics::event(
        "info",
        "learning.summaryGenerated",
        json!({"profileId":profile.id,"scope":profile.scope,"sampleCount":profile.sample_count,"status":"draft"}),
    );
    Ok(profile)
}

#[tauri::command]
pub(crate) fn confirm_preference_summary(app: AppHandle, id: String) -> Result<(), String> {
    let connection = crate::application::history::open(&app)?;
    let target = connection
        .query_row(
            "SELECT scope, app_name FROM preference_profiles WHERE id = ?1 AND status = 'draft'",
            [&id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()
        .map_err(|error| format!("读取偏好总结草稿失败：{error}"))?
        .ok_or("偏好总结草稿不存在")?;
    let transaction = connection
        .unchecked_transaction()
        .map_err(|error| format!("开始确认偏好总结失败：{error}"))?;
    transaction
        .execute(
            "UPDATE preference_profiles SET status = 'superseded'
             WHERE scope = ?1 AND app_name = ?2 AND status = 'active'",
            params![target.0, target.1],
        )
        .map_err(|error| format!("替换旧偏好总结失败：{error}"))?;
    transaction
        .execute(
            "UPDATE preference_profiles SET status = 'active', confirmed_at = ?1 WHERE id = ?2",
            params![now_seconds(), id],
        )
        .map_err(|error| format!("确认偏好总结失败：{error}"))?;
    transaction
        .commit()
        .map_err(|error| format!("提交偏好总结失败：{error}"))?;
    refresh_cache(&app, &connection)?;
    let _ = app.emit(HISTORY_EVENT, json!({"kind":"learningUpdated"}));
    Ok(())
}

#[tauri::command]
pub(crate) fn clear_learning_memory(app: AppHandle) -> Result<(), String> {
    let connection = crate::application::history::open(&app)?;
    connection
        .execute_batch(
            "DELETE FROM history_rule_applications;
             DELETE FROM correction_samples;
             DELETE FROM correction_rules;
             DELETE FROM preference_profiles;
             UPDATE history_entries SET learning_status = 'none', correction_kind = NULL,
               learning_scope = NULL, applied_rule_ids = '[]';",
        )
        .map_err(|error| format!("清空学习记忆失败：{error}"))?;
    refresh_cache(&app, &connection)?;
    let _ = app.emit(HISTORY_EVENT, json!({"kind":"learningCleared"}));
    Ok(())
}

pub(crate) fn cleanup_stale_rules(
    connection: &Connection,
    retention_days: u32,
) -> Result<(), String> {
    let cutoff = now_seconds() - i64::from(retention_days.clamp(1, 3650)) * 86_400;
    connection
        .execute(
            "DELETE FROM correction_rules WHERE COALESCE(last_used_at, updated_at) < ?1",
            [cutoff],
        )
        .map_err(|error| format!("清理过期学习规则失败：{error}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn localized_replacement_is_lexical() {
        let result = classify_correction(
            "请联系开放AI团队处理这个问题",
            "请联系OpenAI团队处理这个问题",
        );
        assert_eq!(result.kind, "lexical");
        assert_eq!(result.candidates.len(), 1);
        assert_eq!(result.candidates[0].before, "开放AI");
        assert_eq!(result.candidates[0].after, "OpenAI");
    }

    #[test]
    fn punctuation_and_format_are_not_lexical() {
        assert_eq!(
            classify_correction("你好,世界", "你好，世界").kind,
            "punctuation"
        );
        assert_eq!(
            classify_correction("你好 世界", "你好\n世界").kind,
            "format"
        );
    }

    #[test]
    fn large_rewrite_is_never_direct_rule() {
        assert_eq!(
            classify_correction("请帮我看看", "麻烦你详细分析一下这个问题并给出完整建议").kind,
            "rewrite"
        );
    }

    #[test]
    fn sensitive_content_is_rejected() {
        assert_eq!(
            classify_correction("账号 123456", "账号 654321").kind,
            "sensitive"
        );
        assert_eq!(
            classify_correction("a@example.com 错", "a@example.com 对").kind,
            "sensitive"
        );
    }

    #[test]
    fn replacement_skips_urls_and_latin_word_internals() {
        assert!(replace_once_safe("https://openai.example", "openai", "OpenAI").is_none());
        assert!(replace_once_safe("`openai`", "openai", "OpenAI").is_none());
        assert!(replace_once_safe("myopenaiapp", "openai", "OpenAI").is_none());
        assert_eq!(
            replace_once_safe("使用 openai", "openai", "OpenAI").as_deref(),
            Some("使用 OpenAI")
        );
    }

    #[test]
    fn normalization_only_changes_rule_key_form() {
        assert_eq!(normalize_rule_text("  OpenAI   Team "), "openai team");
    }

    #[test]
    fn preference_summary_requires_the_strict_bounded_contract() {
        let valid = json!({
            "summary": "表达更简洁",
            "preferences": [{"instruction":"删除重复措辞","evidenceCount":2,"confidence":"high"}],
            "avoid": ["不要扩写"]
        });
        assert_eq!(
            validate_preference_profile(&valid, 3).unwrap(),
            "表达更简洁"
        );
        let invalid = json!({
            "summary": "表达更简洁",
            "preferences": [{"instruction":"删除重复措辞","evidenceCount":4,"confidence":"high"}],
            "avoid": []
        });
        assert!(validate_preference_profile(&invalid, 3).is_err());
    }

    fn learning_connection() -> Connection {
        let connection = Connection::open_in_memory().unwrap();
        connection.execute_batch(
            "CREATE TABLE history_entries (
               id TEXT PRIMARY KEY, learning_status TEXT NOT NULL DEFAULT 'none', learning_scope TEXT
             );
             CREATE TABLE correction_samples (
               id TEXT PRIMARY KEY, entry_id TEXT NOT NULL, before_text TEXT NOT NULL,
               after_text TEXT NOT NULL, app_name TEXT NOT NULL, origin TEXT NOT NULL,
               capture_confidence TEXT NOT NULL, learning_status TEXT NOT NULL,
               correction_kind TEXT NOT NULL, pair_key TEXT NOT NULL, scope TEXT NOT NULL,
               created_at INTEGER NOT NULL, confirmed_at INTEGER
             );
             CREATE TABLE correction_rules (
               id TEXT PRIMARY KEY, pair_key TEXT NOT NULL, rule_key TEXT NOT NULL UNIQUE,
               before_text TEXT NOT NULL, after_text TEXT NOT NULL, app_name TEXT NOT NULL,
               scope TEXT NOT NULL, origin TEXT NOT NULL, status TEXT NOT NULL,
               evidence_count INTEGER NOT NULL, confirmed_count INTEGER NOT NULL,
               negative_count INTEGER NOT NULL, created_at INTEGER NOT NULL,
               updated_at INTEGER NOT NULL, last_used_at INTEGER
             );
             CREATE TABLE preference_profiles (
               id TEXT PRIMARY KEY, scope TEXT NOT NULL, app_name TEXT NOT NULL,
               summary_text TEXT NOT NULL, profile_json TEXT NOT NULL, status TEXT NOT NULL,
               sample_count INTEGER NOT NULL, generation_method TEXT NOT NULL,
               created_at INTEGER NOT NULL, confirmed_at INTEGER
             );
             CREATE TABLE history_rule_applications (
               history_id TEXT NOT NULL, rule_id TEXT NOT NULL, before_text TEXT NOT NULL,
               after_text TEXT NOT NULL, created_at INTEGER NOT NULL,
               negative_feedback_at INTEGER, PRIMARY KEY(history_id, rule_id)
             );",
        ).unwrap();
        connection
    }

    fn insert_evidence(
        connection: &Connection,
        entry_id: &str,
        app_name: &str,
        pair_key: &str,
        origin: &str,
        confidence: &str,
    ) {
        connection
            .execute("INSERT INTO history_entries (id) VALUES (?1)", [entry_id])
            .unwrap();
        connection.execute(
            "INSERT INTO correction_samples
             (id, entry_id, before_text, after_text, app_name, origin, capture_confidence,
              learning_status, correction_kind, pair_key, scope, created_at, confirmed_at)
             VALUES (?1, ?2, '开放AI', 'OpenAI', ?3, ?4, ?5, 'candidate', 'lexical', ?6, 'app', 1, NULL)",
            params![Uuid::new_v4().to_string(), entry_id, app_name, origin, confidence, pair_key],
        ).unwrap();
    }

    #[test]
    fn automatic_lexical_rule_activates_only_after_two_histories() {
        let connection = learning_connection();
        let pair_key = hash_key(&["开放ai", "openai"]);
        insert_evidence(&connection, "one", "Notes", &pair_key, "observed", "high");
        refresh_app_rule(&connection, &pair_key, "Notes", None).unwrap();
        let first: String = connection
            .query_row(
                "SELECT status FROM correction_rules WHERE pair_key = ?1",
                [&pair_key],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(first, "candidate");

        insert_evidence(&connection, "two", "Notes", &pair_key, "observed", "high");
        refresh_app_rule(&connection, &pair_key, "Notes", None).unwrap();
        let second: String = connection
            .query_row(
                "SELECT status FROM correction_rules WHERE pair_key = ?1",
                [&pair_key],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(second, "active");
    }

    #[test]
    fn active_rule_promotes_to_global_after_two_apps() {
        let connection = learning_connection();
        let pair_key = hash_key(&["开放ai", "openai"]);
        insert_evidence(
            &connection,
            "one",
            "Notes",
            &pair_key,
            "manual",
            "confirmed",
        );
        insert_evidence(&connection, "two", "Mail", &pair_key, "manual", "confirmed");
        refresh_app_rule(&connection, &pair_key, "Notes", None).unwrap();
        refresh_app_rule(&connection, &pair_key, "Mail", None).unwrap();
        promote_global_rule(&connection, &pair_key).unwrap();
        let global_count: i64 = connection.query_row(
            "SELECT COUNT(*) FROM correction_rules WHERE pair_key = ?1 AND scope = 'global' AND status = 'active'",
            [&pair_key], |row| row.get(0),
        ).unwrap();
        assert_eq!(global_count, 1);
    }

    #[test]
    fn structured_statistics_are_updated_without_semantic_inference() {
        let connection = learning_connection();
        let pair_key = hash_key(&["开放ai", "openai"]);
        insert_evidence(
            &connection,
            "one",
            "Notes",
            &pair_key,
            "manual",
            "confirmed",
        );
        refresh_statistics(&connection).unwrap();
        let profile: String = connection
            .query_row(
                "SELECT profile_json FROM preference_profiles WHERE id = 'statistics:global'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let profile: Value = serde_json::from_str(&profile).unwrap();
        assert_eq!(profile["evidenceCount"], 1);
        assert_eq!(profile["byKind"]["lexical"], 1);
        assert!(profile.get("userPersonality").is_none());
    }

    #[test]
    fn duplicate_feedback_from_one_history_counts_once_and_two_histories_disable_rule() {
        let connection = learning_connection();
        connection
            .execute(
                "INSERT INTO correction_rules
             (id, pair_key, rule_key, before_text, after_text, app_name, scope, origin, status,
              evidence_count, confirmed_count, negative_count, created_at, updated_at)
             VALUES ('rule', 'pair', 'key', '开放AI', 'OpenAI', 'Notes', 'app', 'manual',
              'active', 2, 1, 0, 1, 1)",
                [],
            )
            .unwrap();
        for history_id in ["one", "two"] {
            connection
                .execute(
                    "INSERT INTO history_rule_applications
                 (history_id, rule_id, before_text, after_text, created_at)
                 VALUES (?1, 'rule', '开放AI', 'OpenAI', 1)",
                    [history_id],
                )
                .unwrap();
        }
        assert!(
            record_negative_feedback_with_connection(&connection, "one", "改回开放AI").unwrap()
        );
        assert!(
            !record_negative_feedback_with_connection(&connection, "one", "改回开放AI").unwrap()
        );
        let once: (i64, String) = connection
            .query_row(
                "SELECT negative_count, status FROM correction_rules WHERE id = 'rule'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(once, (1, "active".into()));
        assert!(
            record_negative_feedback_with_connection(&connection, "two", "再次改回开放AI").unwrap()
        );
        let twice: (i64, String) = connection
            .query_row(
                "SELECT negative_count, status FROM correction_rules WHERE id = 'rule'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(twice, (2, "disabled".into()));
    }
}
