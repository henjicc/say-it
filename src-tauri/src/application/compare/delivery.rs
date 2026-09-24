use super::*;

impl CompareRuntime {
    /// 返回是否归本轮对比所有。查找和状态提交必须在同一把锁内，不能跨 reset 写入新一轮。
    pub(crate) fn record_asr_event(
        &self,
        session_id: &str,
        kind: &str,
        payload: &Value,
    ) -> Result<bool, String> {
        let mut state = self.inner.lock().map_err(|_| "模型对比状态锁失败")?;
        let Some(index) = state.sessions.get(session_id).copied() else {
            return Ok(false);
        };
        if let Some(cell) = state.cells.iter_mut().find(|cell| cell.index == index) {
            match kind {
                "result" => {
                    let segment = payload
                        .get("text")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    cell.status = "streaming".into();
                    // result 是当前一句；每次 final 都提交，合并的仅是界面刷新通知。
                    cell.text.clear();
                    cell.text.push_str(&cell.committed);
                    cell.text.push_str(segment);
                    if payload.get("final").and_then(Value::as_bool) == Some(true) {
                        cell.committed.clone_from(&cell.text);
                    }
                }
                "ended" if cell.status != "error" => cell.status = "done".into(),
                "error" => {
                    cell.status = "error".into();
                    cell.error_message = payload
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("识别失败")
                        .into();
                }
                _ => {}
            }
        }
        if kind == "ended" {
            state.sessions.remove(session_id);
        }
        settle_comparison(&mut state);
        drop(state);
        self.changed.notify_one();
        Ok(true)
    }

    pub(crate) fn register_file_job(
        &self,
        epoch: u64,
        job_id: &str,
        index: usize,
    ) -> Result<(), String> {
        let mut state = self.inner.lock().map_err(|_| "模型对比状态锁失败")?;
        if self.epoch.load(Ordering::Acquire) != epoch {
            return Err("模型对比已取消".into());
        }
        state.jobs.insert(job_id.into(), index);
        Ok(())
    }

    pub(crate) fn record_file_event(
        &self,
        job_id: &str,
        stage: &str,
        payload: &Value,
    ) -> Result<bool, String> {
        let mut state = self.inner.lock().map_err(|_| "模型对比状态锁失败")?;
        let Some(index) = state.jobs.get(job_id).copied() else {
            return Ok(false);
        };
        if let Some(cell) = state.cells.iter_mut().find(|cell| cell.index == index) {
            match stage {
                "uploading" => cell.status = "uploading".into(),
                "submitted" | "polling" => cell.status = "recognizing".into(),
                "completed" => {
                    cell.text = payload
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
                    cell.status = "done".into();
                }
                "error" => {
                    cell.status = "error".into();
                    cell.error_message = payload
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("识别失败")
                        .into();
                }
                _ => {}
            }
        }
        if matches!(stage, "completed" | "error") {
            state.jobs.remove(job_id);
        }
        settle_comparison(&mut state);
        drop(state);
        self.changed.notify_one();
        Ok(true)
    }
}

#[cfg(test)]
mod tests;
