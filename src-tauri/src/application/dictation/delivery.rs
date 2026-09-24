use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Outcome {
    Finalize,
    Fail(String),
}

/// 每个会话只保留第一个有效终态；通知可以合并，终态与原文不能依赖通知载荷。
#[derive(Default)]
pub(super) struct DeliveryState {
    outcome: Option<Outcome>,
    dispatched: bool,
}

impl DeliveryState {
    fn set(&mut self, outcome: Outcome) {
        if self.outcome.is_none() {
            self.outcome = Some(outcome);
        }
    }

    pub(super) fn finish(&mut self) {
        self.set(Outcome::Finalize);
    }

    pub(super) fn failed(&self) -> bool {
        matches!(self.outcome, Some(Outcome::Fail(_)))
    }

    fn take(&mut self) -> Option<Outcome> {
        if self.dispatched {
            return None;
        }
        let outcome = self.outcome.clone()?;
        self.dispatched = true;
        Some(outcome)
    }
}

impl DictationRuntime {
    pub(crate) fn record_asr_event(
        &self,
        session_id: &str,
        kind: &str,
        payload: &Value,
    ) -> Result<bool, String> {
        let mut session = self.session.lock().map_err(|_| "听写状态锁失败")?;
        if session.asr_session_id.as_deref() != Some(session_id) {
            return Ok(false);
        }
        if session.delivery.outcome.is_some()
            || !matches!(
                session.phase,
                DictationPhase::Recording
                    | DictationPhase::WaitingForVoice
                    | DictationPhase::Finishing
            )
        {
            return Ok(true);
        }
        match kind {
            "result" => {
                if let Some(text) = payload.get("text").and_then(Value::as_str) {
                    session.segment.clear();
                    session.segment.push_str(text);
                    if payload.get("final").and_then(Value::as_bool) == Some(true) {
                        commit_current_segment(&mut session);
                    }
                }
            }
            "finish" | "finish_timeout" if session.phase == DictationPhase::Finishing => {
                session.delivery.finish()
            }
            "ended" | "closed" if session.phase == DictationPhase::Finishing => {
                session.delivery.finish()
            }
            "ended" | "closed" => session
                .delivery
                .set(Outcome::Fail("实时语音识别连接意外中断".into())),
            "error" => {
                let message = payload
                    .get("message")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_else(|| payload.to_string());
                session
                    .delivery
                    .set(Outcome::Fail(format!("实时语音识别失败：{message}")));
            }
            _ => {}
        }
        drop(session);
        self.changed.notify_one();
        Ok(true)
    }

    pub(crate) fn record_file_event(
        &self,
        job_id: &str,
        stage: &str,
        payload: &Value,
    ) -> Result<bool, String> {
        let mut session = self.session.lock().map_err(|_| "听写状态锁失败")?;
        if session.file_job_id.as_deref() != Some(job_id) {
            return Ok(false);
        }
        if session.phase != DictationPhase::ProcessingFile || session.delivery.outcome.is_some() {
            return Ok(true);
        }
        match stage {
            "completed" => {
                session.committed = payload
                    .pointer("/result/transcripts")
                    .and_then(Value::as_array)
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(|item| item.get("text").and_then(Value::as_str))
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                    .unwrap_or_default();
                session.delivery.finish();
            }
            "error" => session.delivery.set(Outcome::Fail(
                payload
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("文件识别失败")
                    .into(),
            )),
            _ => return Ok(true),
        }
        drop(session);
        self.changed.notify_one();
        Ok(true)
    }

    fn take_pending(&self) -> Result<Option<(u64, Outcome)>, String> {
        let mut session = self.session.lock().map_err(|_| "听写状态锁失败")?;
        if matches!(
            session.phase,
            DictationPhase::Idle | DictationPhase::Failed | DictationPhase::Injecting
        ) {
            return Ok(None);
        }
        Ok(session
            .delivery
            .take()
            .map(|outcome| (session.epoch, outcome)))
    }
}

pub(super) async fn flush(app: &AppHandle) {
    let pending = match app.state::<RuntimeState>().dictation_runtime.take_pending() {
        Ok(pending) => pending,
        Err(error) => {
            crate::application::diagnostics::event(
                "error",
                "dictation.deliveryFailed",
                json!({"error":error}),
            );
            return;
        }
    };
    match pending {
        Some((epoch, Outcome::Fail(error))) => {
            if let Err(error) = fail(app.clone(), epoch, error).await {
                crate::application::diagnostics::event(
                    "error",
                    "dictation.failureCleanupFailed",
                    json!({"error":error}),
                );
            }
        }
        pending => {
            publish_recognition_state(app);
            if let Some((epoch, Outcome::Finalize)) = pending {
                // 保留历史、后处理、注入和清理的原顺序。慢操作期间新会话数据仍直接入状态。
                finalize(app.clone(), epoch).await;
            }
        }
    }
}

#[cfg(test)]
mod tests;
