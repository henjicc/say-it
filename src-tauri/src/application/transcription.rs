//! 录音识别任务的后端运行时投影。
//!
//! 网络请求仍由迁移期命令适配器执行；该模块先保证 job 的状态和最后结果不再依赖
//! 主窗口监听器。后续复合对齐、缓存与字幕文档会收敛到同一个运行时。
use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::application::contract::{DomainRunState, DomainSnapshot};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TranscriptionJobSnapshot {
    pub(crate) job_id: String,
    pub(crate) stage: String,
    pub(crate) active: bool,
    pub(crate) payload: Value,
}

#[derive(Default)]
pub(crate) struct TranscriptionRuntime {
    jobs: std::sync::Mutex<HashMap<String, TranscriptionJobSnapshot>>,
}

impl TranscriptionRuntime {
    pub(crate) fn apply_event(&self, job_id: &str, stage: &str, payload: Value) {
        let active = !matches!(stage, "completed" | "error");
        if let Ok(mut jobs) = self.jobs.lock() {
            jobs.insert(
                job_id.to_string(),
                TranscriptionJobSnapshot {
                    job_id: job_id.to_string(),
                    stage: stage.to_string(),
                    active,
                    payload,
                },
            );
        }
    }

    pub(crate) fn domain_snapshot(&self) -> DomainSnapshot {
        let Ok(jobs) = self.jobs.lock() else {
            return DomainSnapshot {
                state: DomainRunState::Failed,
                session_id: None,
            };
        };
        let active = jobs.values().find(|job| job.active);
        DomainSnapshot {
            state: if active.is_some() {
                DomainRunState::Running
            } else {
                DomainRunState::Idle
            },
            session_id: active.map(|job| job.job_id.clone()),
        }
    }

    #[allow(dead_code)]
    pub(crate) fn get(&self, job_id: &str) -> Option<TranscriptionJobSnapshot> {
        self.jobs.lock().ok()?.get(job_id).cloned()
    }

    /// 返回全部未过期任务及其最后一次事件。窗口重建时由此恢复投影，
    /// 而不是依赖 WebView 存活期间碰巧收到的事件。
    pub(crate) fn snapshots(&self) -> Vec<TranscriptionJobSnapshot> {
        let Ok(jobs) = self.jobs.lock() else {
            return Vec::new();
        };
        let mut snapshots = jobs.values().cloned().collect::<Vec<_>>();
        snapshots.sort_by(|left, right| left.job_id.cmp(&right.job_id));
        snapshots
    }
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

    #[test]
    fn completed_job_remains_recoverable_but_is_not_running() {
        let runtime = TranscriptionRuntime::default();
        runtime.apply_event(
            "job-1",
            "uploading",
            serde_json::json!({"filePath":"a.wav"}),
        );
        assert_eq!(runtime.domain_snapshot().state, DomainRunState::Running);
        runtime.apply_event(
            "job-1",
            "completed",
            serde_json::json!({"result":{"transcripts":[]}}),
        );
        assert_eq!(runtime.domain_snapshot().state, DomainRunState::Idle);
        assert_eq!(runtime.get("job-1").unwrap().stage, "completed");
    }

    #[test]
    fn snapshots_are_stably_sorted_for_window_recovery() {
        let runtime = TranscriptionRuntime::default();
        runtime.apply_event("job-b", "uploading", serde_json::json!({}));
        runtime.apply_event("job-a", "completed", serde_json::json!({}));
        assert_eq!(
            runtime
                .snapshots()
                .into_iter()
                .map(|job| job.job_id)
                .collect::<Vec<_>>(),
            vec!["job-a", "job-b"]
        );
    }
}
