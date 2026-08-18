use base64::Engine;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use super::model::{
    ActivationTarget, ActiveAppContextExtractionMethod, AppIdentity, CaptureOptions, CaptureStatus,
    CapturedActiveAppContext, ContextSource, OcrTextBlock, DEFAULT_MAX_CAPTURE_SIDE,
};
use super::normalize::enforce_total_budget;
use super::ActiveAppContextProvider;
use crate::providers::capabilities::OcrProvider;

pub(crate) struct MacActiveAppContextProvider;

pub(crate) fn activation_target() -> Option<ActivationTarget> {
    let info = crate::macos_native::frontmost_window().ok()?;
    Some(ActivationTarget {
        window_handle: info.window_id as isize,
        process_id: info.process_id,
        cursor_position: None,
    })
}

pub(crate) fn app_identity(target: ActivationTarget) -> Option<AppIdentity> {
    let info =
        crate::macos_native::window_info(target.window_handle as u32, target.process_id).ok()?;
    Some(AppIdentity {
        process_name: info.process_name,
        app_name: info.app_name,
        window_title: info.window_title,
    })
}

pub(crate) fn list_running_apps() -> Vec<AppIdentity> {
    crate::macos_native::running_apps()
        .unwrap_or_default()
        .into_iter()
        .map(|info| AppIdentity {
            process_name: info.process_name,
            app_name: info.app_name,
            window_title: info.window_title,
        })
        .collect()
}

pub(crate) fn baseline_context(
    target: ActivationTarget,
    blocked_apps: &[String],
    method: ActiveAppContextExtractionMethod,
) -> CapturedActiveAppContext {
    let identity = app_identity(target).unwrap_or(AppIdentity {
        process_name: String::new(),
        app_name: String::new(),
        window_title: None,
    });
    let mut context = CapturedActiveAppContext {
        capture_method: method,
        app_name: identity.app_name,
        process_name: identity.process_name,
        process_id: target.process_id,
        window_title: identity.window_title,
        ..Default::default()
    };
    let process_name = context.process_name.trim().to_lowercase();
    let app_name = context.app_name.trim().to_lowercase();
    if blocked_apps.iter().any(|value| {
        let value = value.trim().to_lowercase();
        !value.is_empty() && (value == process_name || value == app_name)
    }) {
        context.status = CaptureStatus::Blocked;
    }
    context
}

impl ActiveAppContextProvider for MacActiveAppContextProvider {
    fn capture(
        &self,
        target: ActivationTarget,
        blocked_apps: &[String],
        options: CaptureOptions,
        cancelled: &Arc<AtomicBool>,
    ) -> CapturedActiveAppContext {
        let started = Instant::now();
        let mut context = baseline_context(target, blocked_apps, options.method);
        if context.status == CaptureStatus::Blocked {
            context.elapsed_ms = started.elapsed().as_millis() as u64;
            return context;
        }
        if cancelled.load(Ordering::Acquire) || Instant::now() >= options.deadline {
            context.status = CaptureStatus::TimedOut;
            context.elapsed_ms = started.elapsed().as_millis() as u64;
            return context;
        }

        context.status = match options.method {
            ActiveAppContextExtractionMethod::NativeText => {
                context
                    .diagnostics
                    .push("macOS 当前软件上下文请使用“窗口 OCR”；原生文本提取尚未接入。".into());
                if context.use_metadata_fallback("仅使用已取得的应用与窗口信息。") {
                    CaptureStatus::Captured
                } else {
                    CaptureStatus::Unsupported
                }
            }
            ActiveAppContextExtractionMethod::Ocr => {
                capture_ocr(&mut context, target, &options, cancelled)
            }
        };
        enforce_total_budget(&mut context, options.max_chars);
        context.elapsed_ms = started.elapsed().as_millis() as u64;
        context
    }
}

fn capture_ocr(
    context: &mut CapturedActiveAppContext,
    target: ActivationTarget,
    options: &CaptureOptions,
    cancelled: &Arc<AtomicBool>,
) -> CaptureStatus {
    if let Err(error) = crate::macos_native::prepare_context_ocr_permissions(false) {
        context.diagnostics.push(error);
        return CaptureStatus::Failed;
    }
    match crate::macos_native::focused_input_is_secure(target.process_id) {
        Ok(true) => {
            context
                .diagnostics
                .push("焦点位于受保护输入控件，已停止上下文读取。".into());
            return CaptureStatus::Sensitive;
        }
        Err(error) => {
            context.diagnostics.push(error);
            return CaptureStatus::Failed;
        }
        Ok(false) => {}
    }
    let capture_started = Instant::now();
    let max_side = options
        .max_capture_side_override
        .unwrap_or(DEFAULT_MAX_CAPTURE_SIDE);
    let capture = match crate::macos_native::capture_window(target.window_handle as u32, max_side) {
        Ok(capture) => capture,
        Err(error) => {
            context.diagnostics.push(error);
            return CaptureStatus::Failed;
        }
    };
    context.screenshot_width = capture.width;
    context.screenshot_height = capture.height;
    context.screenshot_elapsed_ms = capture_started.elapsed().as_millis() as u64;
    if options.debug {
        context.screenshot_data_url = Some(format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(&capture.png)
        ));
    }
    if cancelled.load(Ordering::Acquire) || Instant::now() >= options.deadline {
        return CaptureStatus::TimedOut;
    }

    let ocr_started = Instant::now();
    let provider_description = options.ocr_provider.description();
    let is_system = matches!(&options.ocr_provider, OcrProvider::System);
    let mut blocks =
        match recognize_with_deadline(&options.ocr_provider, &capture.png, options.deadline) {
            Ok(blocks) => blocks,
            Err(error) if !is_system => {
                context.diagnostics.push(format!(
                    "{provider_description} 失败，已降级到 macOS 系统 OCR：{error}"
                ));
                match recognize_with_deadline(&OcrProvider::System, &capture.png, options.deadline)
                {
                    Ok(blocks) => blocks,
                    Err(fallback_error) => {
                        context
                            .diagnostics
                            .push(format!("macOS 系统 OCR 降级失败：{fallback_error}"));
                        return CaptureStatus::Failed;
                    }
                }
            }
            Err(error) => {
                context.diagnostics.push(error);
                return if Instant::now() >= options.deadline {
                    CaptureStatus::TimedOut
                } else {
                    CaptureStatus::Failed
                };
            }
        };
    context.ocr_elapsed_ms = ocr_started.elapsed().as_millis() as u64;
    if cancelled.load(Ordering::Acquire) || Instant::now() >= options.deadline {
        return CaptureStatus::TimedOut;
    }
    sort_blocks(&mut blocks);
    context.ocr_text = blocks.iter().map(|block| block.text.clone()).collect();
    context.ocr_blocks = blocks;
    if context.ocr_text.is_empty() {
        context
            .diagnostics
            .push("整窗截图成功，但 OCR 没有识别到文字。".into());
        CaptureStatus::Empty
    } else {
        context.source = Some(ContextSource::Ocr);
        CaptureStatus::Captured
    }
}

fn recognize_with_deadline(
    provider: &OcrProvider,
    png: &[u8],
    deadline: Instant,
) -> Result<Vec<OcrTextBlock>, String> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Err("OCR 任务已超时".into());
    }
    tauri::async_runtime::block_on(async {
        tokio::time::timeout(remaining, provider.recognize(png, "activeAppContext"))
            .await
            .map_err(|_| "OCR 任务已超时".to_string())?
    })
}

fn sort_blocks(blocks: &mut [OcrTextBlock]) {
    blocks.sort_by(|left, right| {
        left.bounds
            .top
            .total_cmp(&right.bounds.top)
            .then_with(|| left.bounds.left.total_cmp(&right.bounds.left))
    });
}
