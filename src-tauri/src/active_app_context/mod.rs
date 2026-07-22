mod debug;
mod model;
#[cfg(windows)]
mod native_probe;
mod normalize;
#[cfg(windows)]
mod ocr;
#[cfg(windows)]
mod screen_capture;
#[cfg(not(windows))]
mod unsupported;
#[cfg(windows)]
mod windows;

use model::DICTATION_RESOLVE_WAIT;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::oneshot;
use tokio::sync::oneshot::error::TryRecvError;

const MAX_CONCURRENT_CAPTURES: usize = 4;

pub(crate) use debug::{
    request_debug_capture, reset_debug_capture, set_debug_capture_overrides, DEBUG_STATE_EVENT,
};
pub(crate) use model::{
    ActivationTarget, ActiveAppContextExtractionMethod, ActiveAppContextSummary, AppIdentity,
    CaptureOptions, CaptureStatus, CapturedActiveAppContext, OcrEngineKind,
};

pub(crate) trait ActiveAppContextProvider: Send + Sync + 'static {
    fn capture(
        &self,
        target: ActivationTarget,
        blocked_apps: &[String],
        options: CaptureOptions,
        cancelled: &Arc<AtomicBool>,
    ) -> CapturedActiveAppContext;
}

struct CaptureRequest {
    target: ActivationTarget,
    blocked_apps: Vec<String>,
    options: CaptureOptions,
    cancelled: Arc<AtomicBool>,
    reply: oneshot::Sender<CapturedActiveAppContext>,
}

pub(crate) struct ContextCaptureHandle {
    started: std::time::Instant,
    deadline: std::time::Instant,
    receiver: Option<oneshot::Receiver<CapturedActiveAppContext>>,
    cancelled: Arc<AtomicBool>,
    /// 正文读取在独立线程/进程中执行，超时不能抹去已同步获得的窗口元信息。
    fallback: CapturedActiveAppContext,
}

#[derive(Clone)]
pub(crate) struct ContextCaptureCancellation {
    capture_cancelled: Arc<AtomicBool>,
    session_cancelled: Arc<AtomicBool>,
}

impl ContextCaptureCancellation {
    pub(crate) fn cancel(&self) {
        self.session_cancelled.store(true, Ordering::Release);
        self.capture_cancelled.store(true, Ordering::Release);
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        self.session_cancelled.load(Ordering::Acquire)
    }
}

pub(crate) enum DictationContextCaptureHandle {
    Eager(ContextCaptureHandle),
    #[cfg(windows)]
    DeferredOcr(DeferredOcrCaptureHandle),
}

impl DictationContextCaptureHandle {
    pub(crate) fn cancellation(&self) -> ContextCaptureCancellation {
        let cancelled = match self {
            Self::Eager(handle) => Arc::clone(&handle.cancelled),
            #[cfg(windows)]
            Self::DeferredOcr(handle) => Arc::clone(&handle.cancelled),
        };
        ContextCaptureCancellation {
            capture_cancelled: cancelled,
            session_cancelled: Arc::new(AtomicBool::new(false)),
        }
    }
}

#[cfg(windows)]
pub(crate) struct DeferredOcrCaptureHandle {
    deadline: std::time::Instant,
    receiver: Option<oneshot::Receiver<windows::PreparedOcrCapture>>,
    cancelled: Arc<AtomicBool>,
    fallback: CapturedActiveAppContext,
}

pub(crate) struct ContextCaptureService {
    latest: Arc<Mutex<Option<ActiveAppContextSummary>>>,
    in_flight: Arc<AtomicUsize>,
}

impl Drop for ContextCaptureHandle {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
    }
}

#[cfg(windows)]
impl Drop for DeferredOcrCaptureHandle {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
    }
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
    pub(crate) fn begin_dictation_capture(
        &self,
        target: ActivationTarget,
        blocked_apps: Vec<String>,
        method: ActiveAppContextExtractionMethod,
        ocr_provider: crate::providers::capabilities::OcrProvider,
        defer_ocr: bool,
    ) -> DictationContextCaptureHandle {
        #[cfg(windows)]
        if defer_ocr && method == ActiveAppContextExtractionMethod::Ocr {
            return DictationContextCaptureHandle::DeferredOcr(self.begin_deferred_ocr_capture(
                target,
                blocked_apps,
                ocr_provider,
            ));
        }
        #[cfg(not(windows))]
        let _ = defer_ocr;
        DictationContextCaptureHandle::Eager(self.begin_capture(
            target,
            blocked_apps,
            method,
            ocr_provider,
        ))
    }

    pub(crate) fn begin_capture(
        &self,
        target: ActivationTarget,
        blocked_apps: Vec<String>,
        method: ActiveAppContextExtractionMethod,
        ocr_provider: crate::providers::capabilities::OcrProvider,
    ) -> ContextCaptureHandle {
        self.begin_capture_inner(
            target,
            blocked_apps,
            CaptureOptions::for_method(method, ocr_provider),
        )
    }

    #[cfg(windows)]
    fn begin_deferred_ocr_capture(
        &self,
        target: ActivationTarget,
        blocked_apps: Vec<String>,
        ocr_provider: crate::providers::capabilities::OcrProvider,
    ) -> DeferredOcrCaptureHandle {
        let options =
            CaptureOptions::for_method(ActiveAppContextExtractionMethod::Ocr, ocr_provider);
        let deadline = options.deadline;
        let fallback = windows::baseline_context(target, &blocked_apps, options.method);
        let (reply, receiver) = oneshot::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let Some(permit) = try_acquire_capture(&self.in_flight) else {
            let mut completed = fallback.clone();
            if completed.status != CaptureStatus::Blocked {
                completed.status = CaptureStatus::Failed;
                completed
                    .diagnostics
                    .push("上下文任务繁忙，未执行密码检查与截图。".into());
            }
            let _ = reply.send(windows::PreparedOcrCapture::without_image(
                completed,
                options.ocr_provider,
                options.max_chars,
            ));
            return DeferredOcrCaptureHandle {
                deadline,
                receiver: Some(receiver),
                cancelled,
                fallback,
            };
        };
        let thread_cancelled = Arc::clone(&cancelled);
        let _ = std::thread::Builder::new()
            .name("active-app-context-prepare".into())
            .spawn(move || {
                let prepared =
                    windows::prepare_ocr_capture(target, &blocked_apps, options, &thread_cancelled);
                // 接收方一拿到截图就可能立即申请 OCR 配额；先释放准备阶段配额，
                // 避免并发上限已满时被自己的截图任务挡住。
                drop(permit);
                let _ = reply.send(prepared);
            });
        DeferredOcrCaptureHandle {
            deadline,
            receiver: Some(receiver),
            cancelled,
            fallback,
        }
    }

    pub(crate) fn begin_debug_capture(
        &self,
        target: ActivationTarget,
        blocked_apps: Vec<String>,
        debug_window_handle: Option<isize>,
        method: ActiveAppContextExtractionMethod,
        ocr_provider: crate::providers::capabilities::OcrProvider,
        max_capture_side_override: Option<u32>,
    ) -> ContextCaptureHandle {
        let mut options = CaptureOptions::for_method(method, ocr_provider);
        options.debug = true;
        options.occluding_window_handle = debug_window_handle;
        options.max_capture_side_override = max_capture_side_override;
        self.begin_capture_inner(target, blocked_apps, options)
    }

    fn begin_capture_inner(
        &self,
        target: ActivationTarget,
        blocked_apps: Vec<String>,
        options: CaptureOptions,
    ) -> ContextCaptureHandle {
        let timeout = options.method.timeout();
        let started = options.deadline - timeout;
        let deadline = options.deadline;
        #[cfg(windows)]
        let fallback = windows::baseline_context(target, &blocked_apps, options.method);
        #[cfg(not(windows))]
        let fallback = CapturedActiveAppContext {
            capture_method: options.method,
            process_id: target.process_id,
            ..Default::default()
        };
        let (reply, receiver) = oneshot::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let Some(permit) = try_acquire_capture(&self.in_flight) else {
            let mut fallback = fallback;
            let _ = fallback.use_metadata_fallback("上下文任务繁忙，仅使用基础窗口信息。 ");
            let _ = reply.send(fallback);
            return ContextCaptureHandle {
                started,
                deadline,
                receiver: Some(receiver),
                cancelled,
                fallback: CapturedActiveAppContext::default(),
            };
        };
        let request = CaptureRequest {
            target,
            blocked_apps,
            options,
            cancelled: Arc::clone(&cancelled),
            reply,
        };
        let _ = std::thread::Builder::new()
            .name("active-app-context".into())
            .spawn(move || {
                if request.cancelled.load(Ordering::Acquire) {
                    crate::development_debug_log(
                        "active-app-context",
                        "捕获任务在启动前已取消，跳过平台读取",
                    );
                    return;
                }
                #[cfg(windows)]
                let provider = windows::WindowsActiveAppContextProvider;
                #[cfg(not(windows))]
                let provider = unsupported::UnsupportedActiveAppContextProvider;
                let result = provider.capture(
                    request.target,
                    &request.blocked_apps,
                    request.options,
                    &request.cancelled,
                );
                drop(permit);
                let _ = request.reply.send(result);
            });
        ContextCaptureHandle {
            started,
            deadline,
            receiver: Some(receiver),
            cancelled,
            fallback,
        }
    }

    pub(crate) async fn resolve(&self, handle: ContextCaptureHandle) -> CapturedActiveAppContext {
        let max_wait = handle.deadline.saturating_duration_since(handle.started);
        self.resolve_with_wait(handle, max_wait).await
    }

    pub(crate) async fn resolve_for_dictation(
        &self,
        handle: ContextCaptureHandle,
    ) -> CapturedActiveAppContext {
        self.resolve_with_wait(handle, DICTATION_RESOLVE_WAIT).await
    }

    pub(crate) async fn resolve_dictation_capture(
        &self,
        handle: DictationContextCaptureHandle,
    ) -> CapturedActiveAppContext {
        match handle {
            DictationContextCaptureHandle::Eager(handle) => {
                self.resolve_for_dictation(handle).await
            }
            #[cfg(windows)]
            DictationContextCaptureHandle::DeferredOcr(handle) => {
                self.resolve_deferred_ocr_for_dictation(handle).await
            }
        }
    }

    #[cfg(windows)]
    async fn resolve_deferred_ocr_for_dictation(
        &self,
        mut handle: DeferredOcrCaptureHandle,
    ) -> CapturedActiveAppContext {
        let mut preparation_receiver = handle
            .receiver
            .take()
            .expect("deferred OCR preparation should be resolved only once");
        // 先取已经完成的开始阶段截图。听写本身可能超过准备截止时间，不能因此
        // 丢弃早已准备好的截图；只有尚未完成的准备任务才受旧截止时间约束。
        let preparation_remaining = handle
            .deadline
            .saturating_duration_since(std::time::Instant::now())
            .min(DICTATION_RESOLVE_WAIT);
        let prepared = match preparation_receiver.try_recv() {
            Ok(prepared) => Some(prepared),
            Err(TryRecvError::Closed) => None,
            Err(TryRecvError::Empty) if preparation_remaining.is_zero() => None,
            Err(TryRecvError::Empty) => {
                match tokio::time::timeout(preparation_remaining, &mut preparation_receiver).await {
                    Ok(Ok(prepared)) => Some(prepared),
                    Ok(Err(_)) | Err(_) => None,
                }
            }
        };
        let Some(prepared) = prepared else {
            return unverified_preparation_failure(
                handle.fallback.clone(),
                "OCR 截图准备未及时完成，未使用未经密码检查的窗口信息。",
            );
        };
        if !prepared.has_image() {
            return prepared.into_context();
        }

        let Some(permit) = try_acquire_capture(&self.in_flight) else {
            return metadata_fallback(
                prepared.into_context(),
                "OCR 任务繁忙，仅使用基础窗口信息。",
            );
        };
        // OCR 的总截止从真正提交识别时重新计时，不能复用听写开始时的截图截止。
        let started = std::time::Instant::now();
        let deadline = started + ActiveAppContextExtractionMethod::Ocr.timeout();
        let fallback = prepared.context_for_fallback();
        let (reply, mut receiver) = oneshot::channel();
        let cancelled = Arc::clone(&handle.cancelled);
        let worker_cancelled = Arc::clone(&cancelled);
        let _ = std::thread::Builder::new()
            .name("active-app-context-ocr".into())
            .spawn(move || {
                let result = windows::recognize_prepared_ocr(prepared, deadline, &worker_cancelled);
                drop(permit);
                let _ = reply.send(result);
            });

        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        let mut result = match receiver.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Closed) => metadata_fallback(
                fallback,
                "OCR 工作线程在返回结果前断开，仅使用基础窗口信息。",
            ),
            Err(TryRecvError::Empty) if remaining.is_zero() => {
                metadata_fallback(fallback, "OCR 已到截止，仅使用基础窗口信息。")
            }
            Err(TryRecvError::Empty) => {
                match tokio::time::timeout(remaining, &mut receiver).await {
                    Ok(Ok(result)) => result,
                    Ok(Err(_)) => metadata_fallback(
                        fallback,
                        "OCR 工作线程在返回结果前断开，仅使用基础窗口信息。",
                    ),
                    Err(_) => metadata_fallback(fallback, "OCR 等待超时，仅使用基础窗口信息。"),
                }
            }
        };
        if result.status == CaptureStatus::TimedOut {
            result.elapsed_ms = deadline.saturating_duration_since(started).as_millis() as u64;
        }
        result
    }

    async fn resolve_with_wait(
        &self,
        mut handle: ContextCaptureHandle,
        max_wait: std::time::Duration,
    ) -> CapturedActiveAppContext {
        let started = handle.started;
        let deadline = handle.deadline;
        let fallback = handle.fallback.clone();
        let mut receiver = handle
            .receiver
            .take()
            .expect("context capture handle should be resolved only once");
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
                metadata_fallback(fallback, "上下文工作线程在返回结果前断开，仅使用基础窗口信息。")
            }
            Err(TryRecvError::Empty) if remaining.is_zero() => {
                crate::development_debug_log(
                    "active-app-context",
                    "等待正文时已到总截止且任务尚未完成；本次使用已取得的基础窗口信息",
                );
                metadata_fallback(fallback, "正文读取已到截止，仅使用基础窗口信息。")
            }
            Err(TryRecvError::Empty) => {
                match tokio::time::timeout(remaining, &mut receiver).await {
                    Ok(Ok(result)) => result,
                    Ok(Err(_)) => {
                        crate::development_debug_log(
                            "active-app-context",
                            "上下文工作线程在返回结果前断开",
                        );
                            metadata_fallback(fallback, "上下文工作线程在返回结果前断开，仅使用基础窗口信息。")
                    }
                    Err(_) => {
                        crate::development_debug_log(
                        "active-app-context",
                        format_args!(
                            "本次等待 {} ms 后仍未获得正文；后台上下文任务可能仍在结束，本次使用基础窗口信息",
                            remaining.as_millis(),
                        ),
                    );
                        metadata_fallback(fallback, "正文读取等待超时，仅使用基础窗口信息。")
                    }
                }
            }
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

fn metadata_fallback(
    mut fallback: CapturedActiveAppContext,
    reason: impl Into<String>,
) -> CapturedActiveAppContext {
    if fallback.status == CaptureStatus::Blocked {
        return fallback;
    }
    if !fallback.use_metadata_fallback(reason) {
        fallback.status = CaptureStatus::TimedOut;
    }
    fallback
}

#[cfg(windows)]
fn unverified_preparation_failure(
    mut fallback: CapturedActiveAppContext,
    reason: impl Into<String>,
) -> CapturedActiveAppContext {
    if fallback.status != CaptureStatus::Blocked {
        fallback.status = CaptureStatus::TimedOut;
        fallback.diagnostics.push(reason.into());
    }
    fallback
}

pub(crate) fn configure_native_probe_path(path: PathBuf) {
    #[cfg(windows)]
    native_probe::configure_path(path);
    #[cfg(not(windows))]
    let _ = path;
}

pub(crate) fn shutdown() {
    #[cfg(windows)]
    {
        native_probe::shutdown();
        ocr::shutdown();
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

/// 听写启动时解析前台软件标识。同步、无跨进程调用，可在每次启动时无条件调用。
pub(crate) fn app_identity(target: ActivationTarget) -> Option<AppIdentity> {
    #[cfg(windows)]
    {
        windows::app_identity(target)
    }
    #[cfg(not(windows))]
    {
        unsupported::app_identity(target)
    }
}

/// 当前可切换的顶层窗口列表，供应用规则的软件下拉框使用。
pub(crate) fn list_running_apps() -> Vec<AppIdentity> {
    #[cfg(windows)]
    {
        windows::list_running_apps()
    }
    #[cfg(not(windows))]
    {
        unsupported::list_running_apps()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn expired_capture_returns_timeout_without_updating_latest_summary() {
        let service = ContextCaptureService::default();
        let (_sender, receiver) = oneshot::channel();
        let timeout = ActiveAppContextExtractionMethod::NativeText.timeout();
        let result = service
            .resolve(ContextCaptureHandle {
                started: std::time::Instant::now() - timeout - std::time::Duration::from_millis(1),
                deadline: std::time::Instant::now() - std::time::Duration::from_millis(1),
                receiver: Some(receiver),
                cancelled: Arc::new(AtomicBool::new(false)),
                fallback: CapturedActiveAppContext::default(),
            })
            .await;
        assert_eq!(result.status, CaptureStatus::TimedOut);
        assert_eq!(result.elapsed_ms, timeout.as_millis() as u64);
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
                    - ActiveAppContextExtractionMethod::Ocr.timeout()
                    - std::time::Duration::from_millis(1),
                deadline: std::time::Instant::now() - std::time::Duration::from_millis(1),
                receiver: Some(receiver),
                cancelled: Arc::new(AtomicBool::new(false)),
                fallback: CapturedActiveAppContext::default(),
            })
            .await;

        assert_eq!(result.status, CaptureStatus::Captured);
        assert_eq!(
            result.format_for_prompt(),
            "应用：ChatGPT\n窗口可见文字：已完成的 OCR 内容"
        );
    }

    #[tokio::test]
    async fn timeout_uses_synchronously_captured_window_metadata() {
        let service = ContextCaptureService::default();
        let (_sender, receiver) = oneshot::channel();
        let result = service
            .resolve(ContextCaptureHandle {
                started: std::time::Instant::now(),
                deadline: std::time::Instant::now() + std::time::Duration::from_millis(1),
                receiver: Some(receiver),
                cancelled: Arc::new(AtomicBool::new(false)),
                fallback: CapturedActiveAppContext {
                    app_name: "msedge".into(),
                    window_title: Some("文档 - Microsoft Edge".into()),
                    ..Default::default()
                },
            })
            .await;

        assert_eq!(result.status, CaptureStatus::Captured);
        assert_eq!(
            result.format_for_prompt(),
            "应用：msedge\n窗口：文档 - Microsoft Edge"
        );
    }

    #[test]
    fn dropping_capture_handle_cancels_pending_work() {
        let cancelled = Arc::new(AtomicBool::new(false));
        let (_sender, receiver) = oneshot::channel();
        let handle = ContextCaptureHandle {
            started: std::time::Instant::now(),
            deadline: std::time::Instant::now()
                + ActiveAppContextExtractionMethod::NativeText.timeout(),
            receiver: Some(receiver),
            cancelled: Arc::clone(&cancelled),
            fallback: CapturedActiveAppContext::default(),
        };

        drop(handle);

        assert!(cancelled.load(Ordering::Acquire));
    }

    #[test]
    fn dictation_cancellation_survives_after_the_capture_handle_is_moved() {
        let capture_cancelled = Arc::new(AtomicBool::new(false));
        let (_sender, receiver) = oneshot::channel();
        let handle = DictationContextCaptureHandle::Eager(ContextCaptureHandle {
            started: std::time::Instant::now(),
            deadline: std::time::Instant::now()
                + ActiveAppContextExtractionMethod::NativeText.timeout(),
            receiver: Some(receiver),
            cancelled: Arc::clone(&capture_cancelled),
            fallback: CapturedActiveAppContext::default(),
        });
        let cancellation = handle.cancellation();

        cancellation.cancel();

        assert!(cancellation.is_cancelled());
        assert!(capture_cancelled.load(Ordering::Acquire));
    }

    #[cfg(windows)]
    #[test]
    fn unfinished_password_check_never_promotes_window_metadata() {
        let result = unverified_preparation_failure(
            CapturedActiveAppContext {
                app_name: "password-manager".into(),
                window_title: Some("secret".into()),
                ..Default::default()
            },
            "未完成密码检查",
        );

        assert_eq!(result.status, CaptureStatus::TimedOut);
        assert!(result.format_for_prompt().is_empty());
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn completed_preparation_survives_its_start_deadline() {
        let service = ContextCaptureService::default();
        let (sender, receiver) = oneshot::channel();
        assert!(sender
            .send(windows::PreparedOcrCapture::without_image(
                CapturedActiveAppContext {
                    status: CaptureStatus::Sensitive,
                    capture_method: ActiveAppContextExtractionMethod::Ocr,
                    ..Default::default()
                },
                crate::providers::capabilities::OcrProvider::System,
                model::DEFAULT_MAX_CHARS,
            ))
            .is_ok());

        let result = service
            .resolve_deferred_ocr_for_dictation(DeferredOcrCaptureHandle {
                deadline: std::time::Instant::now() - std::time::Duration::from_millis(1),
                receiver: Some(receiver),
                cancelled: Arc::new(AtomicBool::new(false)),
                fallback: CapturedActiveAppContext::default(),
            })
            .await;

        assert_eq!(result.status, CaptureStatus::Sensitive);
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
