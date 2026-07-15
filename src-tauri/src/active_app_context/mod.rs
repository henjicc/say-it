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
use tokio::sync::oneshot::error::TryRecvError;

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
            mut receiver,
        } = handle;
        let remaining = deadline
            .saturating_duration_since(std::time::Instant::now())
            .min(max_wait);
        crate::development_debug_log(
            "active-app-context",
            format_args!(
                "等待上下文结果：本次最多等待 {} ms，距总截止还剩 {} ms，已运行 {} ms",
                max_wait.as_millis(),
                deadline
                    .saturating_duration_since(std::time::Instant::now())
                    .as_millis(),
                started.elapsed().as_millis(),
            ),
        );
        // 听写可能持续超过捕获硬截止，但 OCR 已在截止前完成并写入通道。
        // 必须先读取已就绪的结果，不能因为“现在已过截止”而错误丢弃它。
        let mut result = match receiver.try_recv() {
            Ok(result) => {
                crate::development_debug_log(
                    "active-app-context",
                    "上下文结果已就绪，即使当前已过总截止仍会用于本次听写",
                );
                result
            }
            Err(TryRecvError::Closed) => {
                crate::development_debug_log(
                    "active-app-context",
                    "上下文工作线程在返回结果前断开",
                );
                CapturedActiveAppContext::with_status(CaptureStatus::Failed)
            }
            Err(TryRecvError::Empty) if remaining.is_zero() => {
                crate::development_debug_log(
                    "active-app-context",
                    "等待上下文时已到总截止且任务尚未完成；本次听写不会使用它",
                );
                CapturedActiveAppContext::with_status(CaptureStatus::TimedOut)
            }
            Err(TryRecvError::Empty) => match tokio::time::timeout(remaining, &mut receiver).await {
                Ok(Ok(result)) => result,
                Ok(Err(_)) => {
                    crate::development_debug_log(
                        "active-app-context",
                        "上下文工作线程在返回结果前断开",
                    );
                    CapturedActiveAppContext::with_status(CaptureStatus::Failed)
                }
                Err(_) => {
                    crate::development_debug_log(
                        "active-app-context",
                        format_args!(
                            "本次等待 {} ms 后仍未获得上下文；后台 OCR 可能仍在运行，本次听写将不使用它",
                            remaining.as_millis(),
                        ),
                    );
                    CapturedActiveAppContext::with_status(CaptureStatus::TimedOut)
                }
            },
        };
        if result.status == CaptureStatus::TimedOut {
            result.elapsed_ms = deadline.saturating_duration_since(started).as_millis() as u64;
        }
        crate::development_debug_log(
            "active-app-context",
            format_args!(
                "上下文解析结果：状态={:?}，返回耗时 {} ms",
                result.status, result.elapsed_ms,
            ),
        );
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

    #[tokio::test]
    async fn completed_capture_is_used_even_if_dictation_finishes_after_deadline() {
        let service = ContextCaptureService::default();
        let (sender, receiver) = oneshot::channel();
        sender
            .send(CapturedActiveAppContext {
                status: CaptureStatus::Captured,
                app_name: "ChatGPT".into(),
                ocr_text: vec!["已完成的 OCR 内容".into()],
                ..Default::default()
            })
            .expect("receiver should accept the completed capture");

        let result = service
            .resolve_for_dictation(ContextCaptureHandle {
                started: std::time::Instant::now()
                    - CAPTURE_TIMEOUT
                    - std::time::Duration::from_millis(1),
                deadline: std::time::Instant::now() - std::time::Duration::from_millis(1),
                receiver,
            })
            .await;

        assert_eq!(result.status, CaptureStatus::Captured);
        assert_eq!(result.format_for_prompt(), "应用：ChatGPT\n窗口可见文字：已完成的 OCR 内容");
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
