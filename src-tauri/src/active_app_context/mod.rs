mod debug;
mod model;
mod normalize;
#[cfg(windows)]
mod ocr;
#[cfg(windows)]
mod screen_capture;
#[cfg(not(windows))]
mod unsupported;
#[cfg(windows)]
mod windows;

use model::{CAPTURE_TIMEOUT, DICTATION_RESOLVE_WAIT};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::oneshot;

const MAX_CONCURRENT_CAPTURES: usize = 4;

pub(crate) use debug::{request_debug_capture, reset_debug_capture, DEBUG_STATE_EVENT};
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
    latest: Arc<Mutex<Option<ActiveAppContextSummary>>>,
    in_flight: Arc<AtomicUsize>,
}

impl Default for ContextCaptureService {
    fn default() -> Self {
        Self {
            latest: Arc::new(Mutex::new(None)),
            in_flight: Arc::new(AtomicUsize::new(0)),
        }
    }
}

struct CapturePermit {
    in_flight: Arc<AtomicUsize>,
}

impl Drop for CapturePermit {
    fn drop(&mut self) {
        self.in_flight.fetch_sub(1, Ordering::AcqRel);
    }
}

fn try_acquire_capture(in_flight: &Arc<AtomicUsize>) -> Option<CapturePermit> {
    in_flight
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
            (current < MAX_CONCURRENT_CAPTURES).then_some(current + 1)
        })
        .ok()?;
    Some(CapturePermit {
        in_flight: Arc::clone(in_flight),
    })
}

impl ContextCaptureService {
    pub(crate) fn begin_capture(
        &self,
        target: ActivationTarget,
        blocked_apps: Vec<String>,
    ) -> ContextCaptureHandle {
        self.begin_capture_inner(target, blocked_apps, CaptureOptions::default())
    }

    pub(crate) fn begin_debug_capture(
        &self,
        target: ActivationTarget,
        blocked_apps: Vec<String>,
        debug_window_handle: Option<isize>,
    ) -> ContextCaptureHandle {
        let mut options = CaptureOptions::default();
        options.debug = true;
        options.occluding_window_handle = debug_window_handle;
        self.begin_capture_inner(target, blocked_apps, options)
    }

    fn begin_capture_inner(
        &self,
        target: ActivationTarget,
        blocked_apps: Vec<String>,
        options: CaptureOptions,
    ) -> ContextCaptureHandle {
        let started = options.deadline - CAPTURE_TIMEOUT;
        let deadline = options.deadline;
        let (reply, receiver) = oneshot::channel();
        let Some(permit) = try_acquire_capture(&self.in_flight) else {
            let _ = reply.send(CapturedActiveAppContext::with_status(
                CaptureStatus::TimedOut,
            ));
            return ContextCaptureHandle {
                started,
                deadline,
                receiver,
            };
        };
        let request = CaptureRequest {
            target,
            blocked_apps,
            options,
            reply,
        };
        let _ = std::thread::Builder::new()
            .name("active-app-context".into())
            .spawn(move || {
                let _permit = permit;
                #[cfg(windows)]
                let provider = windows::WindowsActiveAppContextProvider;
                #[cfg(not(windows))]
                let provider = unsupported::UnsupportedActiveAppContextProvider;
                let result =
                    provider.capture(request.target, &request.blocked_apps, request.options);
                let _ = request.reply.send(result);
            });
        ContextCaptureHandle {
            started,
            deadline,
            receiver,
        }
    }

    pub(crate) async fn resolve(&self, handle: ContextCaptureHandle) -> CapturedActiveAppContext {
        self.resolve_with_wait(handle, CAPTURE_TIMEOUT).await
    }

    pub(crate) async fn resolve_for_dictation(
        &self,
        handle: ContextCaptureHandle,
    ) -> CapturedActiveAppContext {
        self.resolve_with_wait(handle, DICTATION_RESOLVE_WAIT).await
    }

    async fn resolve_with_wait(
        &self,
        handle: ContextCaptureHandle,
        max_wait: std::time::Duration,
    ) -> CapturedActiveAppContext {
        let ContextCaptureHandle {
            started,
            deadline,
            receiver,
        } = handle;
        let remaining = deadline
            .saturating_duration_since(std::time::Instant::now())
            .min(max_wait);
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

pub(crate) fn configure_ocr_model_root(path: PathBuf) {
    #[cfg(windows)]
    ocr::configure_model_root(path);
    #[cfg(not(windows))]
    let _ = path;
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
                started: std::time::Instant::now()
                    - CAPTURE_TIMEOUT
                    - std::time::Duration::from_millis(1),
                deadline: std::time::Instant::now() - std::time::Duration::from_millis(1),
                receiver,
            })
            .await;
        assert_eq!(result.status, CaptureStatus::TimedOut);
        assert_eq!(result.elapsed_ms, CAPTURE_TIMEOUT.as_millis() as u64);
        assert!(service.latest_summary().is_none());
    }

    #[test]
    fn capture_slots_are_bounded_and_released() {
        let in_flight = Arc::new(AtomicUsize::new(0));
        let permits = (0..MAX_CONCURRENT_CAPTURES)
            .map(|_| try_acquire_capture(&in_flight).expect("slot should be available"))
            .collect::<Vec<_>>();
        assert!(try_acquire_capture(&in_flight).is_none());
        drop(permits);
        assert!(try_acquire_capture(&in_flight).is_some());
    }
}
