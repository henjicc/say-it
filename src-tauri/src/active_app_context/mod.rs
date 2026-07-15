mod model;
mod normalize;
#[cfg(not(windows))]
mod unsupported;
#[cfg(windows)]
mod windows;

use model::CAPTURE_TIMEOUT;
use std::sync::{mpsc, Arc, Mutex};
use tokio::sync::oneshot;

pub(crate) use model::{
    ActivationTarget, ActiveAppContextSummary, CaptureOptions, CaptureStatus,
    CapturedActiveAppContext,
};

pub(crate) trait ActiveAppContextProvider: Send + Sync + 'static {
    fn capture(
        &self,
        target: ActivationTarget,
        blocked_apps: &[String],
        options: CaptureOptions,
    ) -> CapturedActiveAppContext;
}

struct CaptureRequest {
    target: ActivationTarget,
    blocked_apps: Vec<String>,
    options: CaptureOptions,
    reply: oneshot::Sender<CapturedActiveAppContext>,
}

pub(crate) struct ContextCaptureHandle {
    started: std::time::Instant,
    deadline: std::time::Instant,
    receiver: oneshot::Receiver<CapturedActiveAppContext>,
}

pub(crate) struct ContextCaptureService {
    sender: mpsc::Sender<CaptureRequest>,
    latest: Arc<Mutex<Option<ActiveAppContextSummary>>>,
}

impl Default for ContextCaptureService {
    fn default() -> Self {
        let (sender, receiver) = mpsc::channel::<CaptureRequest>();
        std::thread::Builder::new()
            .name("active-app-context".into())
            .spawn(move || {
                #[cfg(windows)]
                let provider = windows::WindowsActiveAppContextProvider;
                #[cfg(not(windows))]
                let provider = unsupported::UnsupportedActiveAppContextProvider;

                while let Ok(request) = receiver.recv() {
                    let result =
                        provider.capture(request.target, &request.blocked_apps, request.options);
                    let _ = request.reply.send(result);
                }
            })
            .expect("failed to create active app context worker");
        Self {
            sender,
            latest: Arc::new(Mutex::new(None)),
        }
    }
}

impl ContextCaptureService {
    pub(crate) fn begin_capture(
        &self,
        target: ActivationTarget,
        blocked_apps: Vec<String>,
    ) -> ContextCaptureHandle {
        let options = CaptureOptions::default();
        let started = options.deadline - CAPTURE_TIMEOUT;
        let deadline = options.deadline;
        let (reply, receiver) = oneshot::channel();
        if self
            .sender
            .send(CaptureRequest {
                target,
                blocked_apps,
                options,
                reply,
            })
            .is_err()
        {
            let (fallback_reply, fallback_receiver) = oneshot::channel();
            let _ =
                fallback_reply.send(CapturedActiveAppContext::with_status(CaptureStatus::Failed));
            return ContextCaptureHandle {
                started,
                deadline,
                receiver: fallback_receiver,
            };
        }
        ContextCaptureHandle {
            started,
            deadline,
            receiver,
        }
    }

    pub(crate) async fn resolve(&self, handle: ContextCaptureHandle) -> CapturedActiveAppContext {
        let ContextCaptureHandle {
            started,
            deadline,
            receiver,
        } = handle;
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        let mut result = if remaining.is_zero() {
            CapturedActiveAppContext::with_status(CaptureStatus::TimedOut)
        } else {
            match tokio::time::timeout(remaining, receiver).await {
                Ok(Ok(result)) => result,
                Ok(Err(_)) => CapturedActiveAppContext::with_status(CaptureStatus::Failed),
                Err(_) => CapturedActiveAppContext::with_status(CaptureStatus::TimedOut),
            }
        };
        if result.status == CaptureStatus::TimedOut {
            result.elapsed_ms = deadline.saturating_duration_since(started).as_millis() as u64;
        }
        result
    }

    pub(crate) fn remember(&self, context: &CapturedActiveAppContext) {
        if let Ok(mut latest) = self.latest.lock() {
            *latest = Some(ActiveAppContextSummary::from(context));
        }
    }

    pub(crate) fn latest_summary(&self) -> Option<ActiveAppContextSummary> {
        self.latest.lock().ok().and_then(|value| value.clone())
    }
}

pub(crate) fn activation_target() -> Option<ActivationTarget> {
    #[cfg(windows)]
    {
        windows::activation_target()
    }
    #[cfg(not(windows))]
    {
        unsupported::activation_target()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn expired_capture_returns_timeout_without_updating_latest_summary() {
        let service = ContextCaptureService::default();
        let (_sender, receiver) = oneshot::channel();
        let result = service
            .resolve(ContextCaptureHandle {
                started: std::time::Instant::now() - std::time::Duration::from_millis(801),
                deadline: std::time::Instant::now() - std::time::Duration::from_millis(1),
                receiver,
            })
            .await;
        assert_eq!(result.status, CaptureStatus::TimedOut);
        assert_eq!(result.elapsed_ms, CAPTURE_TIMEOUT.as_millis() as u64);
        assert!(service.latest_summary().is_none());
    }
}
