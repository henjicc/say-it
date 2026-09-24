//! 录音识别任务的后端运行时投影。
//!
//! 网络请求仍由迁移期命令适配器执行；该模块先保证 job 的状态和最后结果不再依赖
//! 主窗口监听器。后续复合对齐、缓存与字幕文档会收敛到同一个运行时。
use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::application::contract::{DomainRunState, DomainSnapshot};

/// 一个识别任务是为谁跑的。
///
/// 字幕转写、文稿对齐、模型对比、file 模式听写走的是同一条 `transcription_start`，
/// 但它们属于完全不同的界面。用途必须由后端记住：前端的归属信息只在内存里，
/// 主窗口重建后就没了，恢复时会把别人的任务当成自己的投影出来。
pub(crate) const TRANSCRIPTION_JOB_KINDS: &[&str] =
    &["transcribe", "align", "compare", "dictation"];
pub(crate) const DEFAULT_TRANSCRIPTION_JOB_KIND: &str = "transcribe";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TranscriptionJobSnapshot {
    pub(crate) job_id: String,
    pub(crate) kind: String,
    pub(crate) stage: String,
    pub(crate) active: bool,
    pub(crate) payload: Value,
}

#[derive(Default)]
pub(crate) struct TranscriptionRuntime {
    inner: std::sync::Mutex<RuntimeProjection>,
}

#[derive(Default)]
struct RuntimeProjection {
    sequence: u64,
    registered: HashMap<String, (String, u64)>,
    jobs: HashMap<String, (u64, TranscriptionJobSnapshot)>,
    latest: HashMap<String, String>,
}

impl TranscriptionRuntime {
    /// 在任务发出第一个事件之前登记它的用途。
    pub(crate) fn register(&self, job_id: &str, kind: &str) -> Result<(), String> {
        let mut inner = self.inner.lock().map_err(|_| "录音识别投影锁定失败")?;
        if inner.registered.contains_key(job_id) {
            return Ok(());
        }
        let kind = normalize_job_kind(kind).to_string();
        inner.sequence = inner
            .sequence
            .checked_add(1)
            .ok_or("录音识别任务序号已耗尽")?;
        let order = inner.sequence;
        inner
            .registered
            .insert(job_id.to_string(), (kind.clone(), order));
        if matches!(kind.as_str(), "transcribe" | "align") {
            if let Some(previous) = inner.latest.insert(kind, job_id.to_string()) {
                if inner
                    .jobs
                    .get(&previous)
                    .is_some_and(|(_, job)| !job.active)
                {
                    inner.jobs.remove(&previous);
                }
            }
        }
        Ok(())
    }

    /// 返回完整事件供现有订阅方消费；只复制窗口恢复确实需要的载荷。
    pub(crate) fn apply_event(
        &self,
        job_id: &str,
        stage: &str,
        payload: Value,
    ) -> Result<Option<Value>, String> {
        let mut inner = self.inner.lock().map_err(|_| "录音识别投影锁定失败")?;
        // 实际任务退出后，迟到的取消/回调不能重新创建缓存或丢失原用途。
        let Some((kind, order)) = inner.registered.get(job_id).cloned() else {
            return Ok(None);
        };
        let mut payload = match payload {
            Value::Object(map) => Value::Object(map),
            other => serde_json::json!({ "data": other }),
        };
        let map = payload.as_object_mut().unwrap();
        map.insert("jobId".into(), job_id.into());
        map.insert("stage".into(), stage.into());
        map.insert("kind".into(), kind.clone().into());
        let active = !matches!(stage, "completed" | "error");
        let recoverable = inner.latest.get(&kind).is_some_and(|id| id == job_id);
        if active || recoverable {
            inner.jobs.insert(
                job_id.to_string(),
                (
                    order,
                    TranscriptionJobSnapshot {
                        job_id: job_id.to_string(),
                        kind,
                        stage: stage.to_string(),
                        active,
                        payload: payload.clone(),
                    },
                ),
            );
        } else {
            inner.jobs.remove(job_id);
        }
        Ok(Some(payload))
    }

    /// 必须在工作实际退出后调用，不能在取消标记置位时提前注销。
    pub(crate) fn finish(&self, job_id: &str) -> Result<(), String> {
        let mut inner = self.inner.lock().map_err(|_| "录音识别投影锁定失败")?;
        inner.registered.remove(job_id);
        if !inner.latest.values().any(|id| id == job_id) {
            inner.jobs.remove(job_id);
        }
        Ok(())
    }

    pub(crate) fn domain_snapshot(&self) -> DomainSnapshot {
        let Ok(inner) = self.inner.lock() else {
            return DomainSnapshot {
                state: DomainRunState::Failed,
                session_id: None,
            };
        };
        let active = inner
            .jobs
            .values()
            .filter(|(_, job)| job.active)
            .max_by_key(|(order, _)| order)
            .map(|(_, job)| job);
        DomainSnapshot {
            state: if active.is_some() {
                DomainRunState::Running
            } else {
                DomainRunState::Idle
            },
            session_id: active.map(|job| job.job_id.clone()),
        }
    }

    #[cfg(test)]
    pub(crate) fn get(&self, job_id: &str) -> Option<TranscriptionJobSnapshot> {
        self.inner
            .lock()
            .ok()?
            .jobs
            .get(job_id)
            .map(|(_, job)| job.clone())
    }

    /// 返回运行中任务和每个页面最新结果，按登记先后排序。窗口重建时由此恢复投影，
    /// 而不是依赖 WebView 存活期间碰巧收到的事件。
    pub(crate) fn snapshots(&self) -> Vec<TranscriptionJobSnapshot> {
        let Ok(inner) = self.inner.lock() else {
            return Vec::new();
        };
        let mut snapshots = inner.jobs.values().collect::<Vec<_>>();
        snapshots.sort_by_key(|(order, _)| order);
        snapshots.into_iter().map(|(_, job)| job.clone()).collect()
    }
}

/// 未知用途一律归为字幕转写（历史上唯一的用途），不自己发明新值。
pub(crate) fn normalize_job_kind(kind: &str) -> &str {
    let kind = kind.trim();
    TRANSCRIPTION_JOB_KINDS
        .iter()
        .find(|known| **known == kind)
        .copied()
        .unwrap_or(DEFAULT_TRANSCRIPTION_JOB_KIND)
}

#[tauri::command]
pub(crate) fn get_transcription_runtime(
    state: tauri::State<'_, crate::state::RuntimeState>,
) -> Vec<TranscriptionJobSnapshot> {
    state.transcription_runtime.snapshots()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 任务归属必须由后端记住。
    ///
    /// 前端以前只在内存里记了一个 alignJobId，`destroy_main_window` 后重建窗口就没了，
    /// 恢复时把文稿对齐（以及模型对比、file 模式听写）的任务当成普通转写投影到字幕转写页。
    #[test]
    fn a_job_keeps_the_kind_it_was_registered_with() {
        let runtime = TranscriptionRuntime::default();
        runtime.register("align-1", "align").unwrap();
        runtime.register("job-1", "transcribe").unwrap();
        runtime
            .apply_event("align-1", "polling", serde_json::json!({}))
            .unwrap();
        runtime
            .apply_event("job-1", "polling", serde_json::json!({}))
            .unwrap();

        assert_eq!(runtime.get("align-1").unwrap().kind, "align");
        assert_eq!(runtime.get("job-1").unwrap().kind, "transcribe");
    }

    /// 未知用途仍归为字幕转写；未知任务不接受事件，避免已清理任务复活。
    #[test]
    fn unknown_kinds_fall_back_to_transcribe() {
        let runtime = TranscriptionRuntime::default();
        runtime.register("job-2", "乱写的").unwrap();
        runtime
            .apply_event("job-2", "polling", serde_json::json!({}))
            .unwrap();
        assert_eq!(runtime.get("job-2").unwrap().kind, "transcribe");

        let runtime = TranscriptionRuntime::default();
        assert!(runtime
            .apply_event("job-3", "polling", serde_json::json!({}))
            .unwrap()
            .is_none());
        assert!(runtime.get("job-3").is_none());
    }

    #[test]
    fn completed_job_remains_recoverable_but_is_not_running() {
        let runtime = TranscriptionRuntime::default();
        runtime.register("job-1", "transcribe").unwrap();
        runtime
            .apply_event(
                "job-1",
                "uploading",
                serde_json::json!({"filePath":"a.wav"}),
            )
            .unwrap();
        assert_eq!(runtime.domain_snapshot().state, DomainRunState::Running);
        runtime
            .apply_event(
                "job-1",
                "completed",
                serde_json::json!({"result":{"transcripts":[]}}),
            )
            .unwrap();
        runtime.finish("job-1").unwrap();
        assert_eq!(runtime.domain_snapshot().state, DomainRunState::Idle);
        assert_eq!(runtime.get("job-1").unwrap().stage, "completed");
    }

    #[test]
    fn snapshots_are_stably_sorted_for_window_recovery() {
        let runtime = TranscriptionRuntime::default();
        runtime.register("job-b", "transcribe").unwrap();
        runtime.register("job-a", "transcribe").unwrap();
        runtime
            .apply_event("job-b", "uploading", serde_json::json!({}))
            .unwrap();
        runtime
            .apply_event("job-a", "completed", serde_json::json!({}))
            .unwrap();
        assert_eq!(
            runtime
                .snapshots()
                .into_iter()
                .map(|job| job.job_id)
                .collect::<Vec<_>>(),
            vec!["job-b", "job-a"]
        );
    }
}

#[cfg(test)]
mod retention_tests;
