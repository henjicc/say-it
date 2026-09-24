use crate::state::AsrStreamHandle;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// 音频入口已准备，但尚未创建识别线程/运行时。调用方登记路由之后才能启动。
#[must_use = "登记业务会话和音频路由后调用 start；丢弃会取消准备的连接"]
pub(crate) struct PreparedAsrStream {
    pub(crate) session_id: String,
    streams: Arc<Mutex<HashMap<String, AsrStreamHandle>>>,
    launch: Option<Box<dyn FnOnce() -> Result<(), String> + Send>>,
    started: bool,
}

impl PreparedAsrStream {
    pub(super) fn new(
        session_id: String,
        streams: Arc<Mutex<HashMap<String, AsrStreamHandle>>>,
        launch: impl FnOnce() -> Result<(), String> + Send + 'static,
    ) -> Self {
        Self {
            session_id,
            streams,
            launch: Some(Box::new(launch)),
            started: false,
        }
    }

    pub(crate) fn start(mut self) -> Result<(), String> {
        let ready = self
            .streams
            .lock()
            .map_err(|_| "ASR stream lock failed")?
            .get(&self.session_id)
            .is_some_and(|handle| !handle.tx.is_closed());
        if !ready {
            return Err("识别启动已取消".into());
        }
        // 只能消费一次；失败仍由 Drop 清理已登记的音频入口与未启动资源。
        self.launch
            .take()
            .expect("prepared ASR launch consumed once")()?;
        self.started = true;
        Ok(())
    }
}

impl Drop for PreparedAsrStream {
    fn drop(&mut self) {
        if self.started {
            return;
        }
        match self.streams.lock() {
            Ok(mut streams) => {
                if let Some(handle) = streams.remove(&self.session_id) {
                    handle.stop();
                }
            }
            Err(error) => {
                crate::application::diagnostics::event(
                    "error",
                    "asr.startupCleanupFailed",
                    serde_json::json!({"error":error.to_string()}),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests;
