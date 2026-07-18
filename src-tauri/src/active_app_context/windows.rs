use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use windows::core::PWSTR;
use windows::Win32::Foundation::{CloseHandle, POINT, HWND};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Accessibility::{CUIAutomation, IUIAutomation};
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetForegroundWindow, GetWindowTextLengthW, GetWindowTextW,
    GetWindowThreadProcessId,
};

use super::model::{
    ActivationTarget, ActiveAppContextExtractionMethod, CaptureOptions, CaptureStatus,
    CapturedActiveAppContext, ContextSource, OcrEngineKind,
};
use super::normalize::{enforce_total_budget, normalize_text};
use super::{native_probe, ocr, screen_capture, ActiveAppContextProvider};

pub(crate) struct WindowsActiveAppContextProvider;

pub(crate) fn activation_target() -> Option<ActivationTarget> {
    let window = unsafe { GetForegroundWindow() };
    if window.0.is_null() {
        return None;
    }
    let mut process_id = 0u32;
    unsafe { GetWindowThreadProcessId(window, Some(&mut process_id)) };
    if process_id == 0 || process_id == std::process::id() {
        return None;
    }
    let mut cursor = POINT::default();
    let cursor_position = unsafe { GetCursorPos(&mut cursor) }
        .is_ok()
        .then_some((cursor.x, cursor.y));
    Some(ActivationTarget {
        window_handle: window.0 as isize,
        process_id,
        cursor_position,
    })
}

/// 只读取本进程可直接取得的窗口元信息。它必须在探针或 UIA 发生跨进程调用前完成，
/// 以便正文读取超时仍有稳定的场景保底。
pub(crate) fn baseline_context(
    target: ActivationTarget,
    blocked_apps: &[String],
    method: ActiveAppContextExtractionMethod,
) -> CapturedActiveAppContext {
    let process_name = process_name(target.process_id).unwrap_or_default();
    let app_name = Path::new(&process_name)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(&process_name)
        .to_string();
    let mut context = CapturedActiveAppContext {
        capture_method: method,
        app_name,
        process_name,
        process_id: target.process_id,
        window_title: window_title(target.window_handle),
        ..Default::default()
    };
    if is_blocked(&context, blocked_apps) {
        context.status = CaptureStatus::Blocked;
    }
    context
}

impl ActiveAppContextProvider for WindowsActiveAppContextProvider {
    fn capture(
        &self,
        target: ActivationTarget,
        blocked_apps: &[String],
        options: CaptureOptions,
        cancelled: &Arc<AtomicBool>,
    ) -> CapturedActiveAppContext {
        let started = Instant::now();
        let mut context = baseline_context(target, blocked_apps, options.method);

        crate::development_debug_log(
            "active-app-context",
            format_args!(
                "开始捕获：HWND=0x{:X}，PID={}，应用={}，方式={:?}",
                target.window_handle, target.process_id, context.app_name, options.method,
            ),
        );

        if cancelled.load(Ordering::Acquire) {
            context.status = CaptureStatus::TimedOut;
            context.elapsed_ms = started.elapsed().as_millis() as u64;
            return context;
        }

        if context.status == CaptureStatus::Blocked {
            context.elapsed_ms = started.elapsed().as_millis() as u64;
            crate::development_debug_log(
                "active-app-context",
                format_args!("捕获已拦截：黑名单应用 {}", context.process_name),
            );
            return context;
        }
        if expired(options.deadline) {
            context.status = CaptureStatus::TimedOut;
            context.elapsed_ms = started.elapsed().as_millis() as u64;
            return context;
        }

        context.status = match options.method {
            // 原生探针会在读取正文前自行检查密码控件。这里不重复执行全局 UIA
            // GetFocusedElement，因为该调用在 Chromium/Electron 上可能耗尽整个 800ms 配额。
            ActiveAppContextExtractionMethod::NativeText => match native_probe::capture(
                    target,
                    &mut context,
                    options.deadline,
                    options.max_chars,
                    cancelled,
                ) {
                Ok(status) => status,
                Err(error) => {
                    context.diagnostics.push(error);
                    if cancelled.load(Ordering::Acquire) || expired(options.deadline) {
                        CaptureStatus::TimedOut
                    } else {
                        CaptureStatus::Failed
                    }
                }
            },
            ActiveAppContextExtractionMethod::Ocr => {
                match focused_target_is_password(target) {
                    Ok(true) => {
                        context
                            .diagnostics
                            .push("焦点位于受保护输入控件，已停止上下文读取。".into());
                        CaptureStatus::Sensitive
                    }
                    Ok(false) => match capture_and_recognize(
                        &mut context,
                        target.window_handle,
                        options.debug,
                        options.occluding_window_handle,
                        options.deadline,
                        cancelled,
                        options.ocr_engine,
                        options.max_capture_side_override,
                    ) {
                        Ok(()) if context.ocr_text.is_empty() => {
                            context
                                .diagnostics
                                .push("整窗截图成功，但 OCR 没有识别到文字。".into());
                            CaptureStatus::Empty
                        }
                        Ok(()) => {
                            context.source = Some(ContextSource::Ocr);
                            CaptureStatus::Captured
                        }
                        Err(error) => {
                            context.diagnostics.push(error);
                            if cancelled.load(Ordering::Acquire) || expired(options.deadline) {
                                CaptureStatus::TimedOut
                            } else {
                                CaptureStatus::Failed
                            }
                        }
                    },
                    Err(error) => {
                        context.diagnostics.push(error);
                        if cancelled.load(Ordering::Acquire) || expired(options.deadline) {
                            CaptureStatus::TimedOut
                        } else {
                            CaptureStatus::Failed
                        }
                    }
                }
            }
        };
        if options.method == ActiveAppContextExtractionMethod::NativeText
            && matches!(
                context.status,
                CaptureStatus::Empty | CaptureStatus::TimedOut | CaptureStatus::Failed
            )
        {
            let status = context.status;
            let _ = context.use_metadata_fallback(format!(
                "原生文本读取{status:?}，仅使用已取得的应用与窗口信息。"
            ));
        }
        enforce_total_budget(&mut context, options.max_chars);
        context.elapsed_ms = started.elapsed().as_millis() as u64;
        crate::development_debug_log(
            "active-app-context",
            format_args!(
                "捕获结束：状态={:?}，截图 {}×{}（{} ms），OCR {} ms，总计 {} ms\n--- 最终上下文开始 ---\n{}\n--- 最终上下文结束 ---",
                context.status,
                context.screenshot_width,
                context.screenshot_height,
                context.screenshot_elapsed_ms,
                context.ocr_elapsed_ms,
                context.elapsed_ms,
                context.format_for_prompt(),
            ),
        );
        context
    }
}

fn capture_and_recognize(
    context: &mut CapturedActiveAppContext,
    window_handle: isize,
    debug: bool,
    occluding_window_handle: Option<isize>,
    deadline: Instant,
    cancelled: &Arc<AtomicBool>,
    ocr_engine: OcrEngineKind,
    max_capture_side_override: Option<u32>,
) -> Result<(), String> {
    let captured = screen_capture::capture_window(
        window_handle,
        occluding_window_handle,
        max_capture_side_override,
    )?;
    crate::development_debug_log(
        "active-app-context",
        format_args!(
            "截图完成：{}×{}，耗时 {} ms；准备提交 OCR",
            captured.image.width(),
            captured.image.height(),
            captured.elapsed_ms,
        ),
    );
    context.screenshot_width = captured.image.width();
    context.screenshot_height = captured.image.height();
    context.screenshot_elapsed_ms = captured.elapsed_ms;
    let debug_image = debug.then(|| captured.image.clone());

    if cancelled.load(Ordering::Acquire) {
        return Err("上下文捕获已取消".into());
    }
    let output_result =
        ocr::run_full_window(ocr_engine, captured.image, deadline, Arc::clone(cancelled));
    if let Some(image) = debug_image {
        context.screenshot_data_url = ocr::png_data_url(&image).ok();
    }
    let output = output_result?;
    context.ocr_text = output.text;
    context.ocr_blocks = output.blocks;
    context.model_init_ms = output.model_init_ms;
    context.ocr_elapsed_ms = output.elapsed_ms;
    context.truncated |= output.truncated;
    Ok(())
}

fn is_blocked(context: &CapturedActiveAppContext, blocked_apps: &[String]) -> bool {
    let process_name = context.process_name.to_lowercase();
    let app_name = context.app_name.to_lowercase();
    blocked_apps.iter().any(|blocked| {
        let blocked = blocked.trim().to_lowercase();
        !blocked.is_empty() && (blocked == process_name || blocked == app_name)
    })
}

fn expired(deadline: Instant) -> bool {
    Instant::now() >= deadline
}

fn focused_target_is_password(target: ActivationTarget) -> Result<bool, String> {
    unsafe {
        let initialized_here = CoInitializeEx(None, COINIT_MULTITHREADED).is_ok();
        let result = (|| {
            let automation: IUIAutomation =
                CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
                    .map_err(|error| format!("无法初始化 UI Automation 密码检查：{error}"))?;
            let focused = automation
                .GetFocusedElement()
                .map_err(|error| format!("无法读取焦点控件用于密码检查：{error}"))?;
            let process_id = focused
                .CurrentProcessId()
                .map_err(|error| format!("无法确认焦点控件进程：{error}"))?;
            if process_id != target.process_id as i32 {
                return Ok(false);
            }
            focused
                .CurrentIsPassword()
                .map(|value| value.as_bool())
                .map_err(|error| format!("无法检查焦点控件是否受保护：{error}"))
        })();
        if initialized_here {
            CoUninitialize();
        }
        result
    }
}

fn process_name(process_id: u32) -> Option<String> {
    let process =
        unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id).ok()? };
    let mut buffer = vec![0u16; 32_768];
    let mut length = buffer.len() as u32;
    let result = unsafe {
        QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut length,
        )
    };
    unsafe {
        let _ = CloseHandle(process);
    }
    if result.is_err() || length == 0 {
        return None;
    }
    let path = String::from_utf16_lossy(&buffer[..length as usize]);
    Path::new(&path)
        .file_name()
        .and_then(|value| value.to_str())
        .map(str::to_string)
}

fn window_title(window_handle: isize) -> Option<String> {
    let window = HWND(window_handle as *mut std::ffi::c_void);
    let length = unsafe { GetWindowTextLengthW(window) };
    if length <= 0 {
        return None;
    }
    let mut buffer = vec![0u16; length as usize + 1];
    let copied = unsafe { GetWindowTextW(window, &mut buffer) };
    if copied <= 0 {
        return None;
    }
    let title = normalize_text(&String::from_utf16_lossy(&buffer[..copied as usize]));
    (!title.is_empty()).then_some(title)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blacklist_matches_process_or_display_name_case_insensitively() {
        let context = CapturedActiveAppContext {
            process_name: "SecretApp.exe".into(),
            app_name: "SecretApp".into(),
            ..Default::default()
        };
        assert!(is_blocked(&context, &["secretapp.exe".into()]));
        assert!(is_blocked(&context, &["SECRETAPP".into()]));
        assert!(!is_blocked(&context, &["notepad.exe".into()]));
    }
}
