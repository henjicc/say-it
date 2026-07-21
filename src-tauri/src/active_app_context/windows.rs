use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use windows::core::PWSTR;
use windows::Win32::Foundation::{CloseHandle, BOOL, HWND, LPARAM, POINT};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Accessibility::{CUIAutomation, IUIAutomation};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumChildWindows, EnumWindows, GetClassNameW, GetCursorPos, GetForegroundWindow, GetWindow,
    GetWindowLongPtrW, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId,
    IsWindowVisible, GWL_EXSTYLE, GW_OWNER, WS_EX_TOOLWINDOW,
};

use super::model::{
    ActivationTarget, ActiveAppContextExtractionMethod, AppIdentity, CaptureOptions, CaptureStatus,
    CapturedActiveAppContext, ContextSource,
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
                        options.ocr_provider,
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
    ocr_provider: crate::providers::capabilities::OcrProvider,
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
        ocr::run_full_window(ocr_provider, captured.image, deadline, Arc::clone(cancelled));
    if let Some(image) = debug_image {
        context.screenshot_data_url = ocr::png_data_url(&image).ok();
    }
    let output = output_result?;
    if let Some(diagnostic) = output.diagnostic {
        context.diagnostics.push(diagnostic);
    }
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

/// UWP / 打包应用的顶层窗口由 `ApplicationFrameHost.exe` 托管，直接取 PID 会让所有
/// UWP 应用都显示成同一个进程名，规则互相串。真实进程挂在 `CoreWindow` 子窗口上。
const UWP_FRAME_HOST: &str = "applicationframehost.exe";
const UWP_CORE_WINDOW_CLASS: &str = "Windows.UI.Core.CoreWindow";

fn class_name(window: HWND) -> String {
    let mut buffer = [0u16; 256];
    let copied = unsafe { GetClassNameW(window, &mut buffer) };
    if copied <= 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buffer[..copied as usize])
}

unsafe extern "system" fn collect_core_window(window: HWND, lparam: LPARAM) -> BOOL {
    let found = &mut *(lparam.0 as *mut Option<u32>);
    if class_name(window) != UWP_CORE_WINDOW_CLASS {
        return BOOL(1);
    }
    let mut process_id = 0u32;
    GetWindowThreadProcessId(window, Some(&mut process_id));
    if process_id != 0 {
        *found = Some(process_id);
        return BOOL(0);
    }
    BOOL(1)
}

/// 若窗口是 UWP 框架宿主，返回承载真实应用的子窗口 PID；否则返回原 PID。
fn resolve_real_process(window: HWND, process_id: u32) -> u32 {
    let host = process_name(process_id).unwrap_or_default().to_lowercase();
    if host != UWP_FRAME_HOST {
        return process_id;
    }
    let mut found: Option<u32> = None;
    let _ = unsafe {
        EnumChildWindows(
            window,
            Some(collect_core_window),
            LPARAM(&mut found as *mut Option<u32> as isize),
        )
    };
    // 子窗口自身可能仍属于宿主进程，那种情况下没有更好的答案，保持原 PID。
    found.filter(|pid| *pid != process_id).unwrap_or(process_id)
}

fn identity_for(window: HWND, process_id: u32) -> AppIdentity {
    let process_name = process_name(process_id).unwrap_or_default();
    let app_name = Path::new(&process_name)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(&process_name)
        .to_string();
    AppIdentity {
        process_name,
        app_name,
        window_title: window_title(window.0 as isize),
    }
}

pub(crate) fn app_identity(target: ActivationTarget) -> Option<AppIdentity> {
    let window = HWND(target.window_handle as *mut std::ffi::c_void);
    let process_id = resolve_real_process(window, target.process_id);
    let identity = identity_for(window, process_id);
    (!identity.process_name.is_empty()).then_some(identity)
}

unsafe extern "system" fn collect_running_app(window: HWND, lparam: LPARAM) -> BOOL {
    let apps = &mut *(lparam.0 as *mut Vec<AppIdentity>);
    // 只要用户能看见、能切换过去的窗口：可见、非工具窗口、非属主窗口的附属窗口、有标题。
    if !IsWindowVisible(window).as_bool() {
        return BOOL(1);
    }
    let ex_style = GetWindowLongPtrW(window, GWL_EXSTYLE) as u32;
    if ex_style & WS_EX_TOOLWINDOW.0 != 0 {
        return BOOL(1);
    }
    if GetWindow(window, GW_OWNER).map(|owner| !owner.0.is_null()).unwrap_or(false) {
        return BOOL(1);
    }
    if window_title(window.0 as isize).is_none() {
        return BOOL(1);
    }
    let mut process_id = 0u32;
    GetWindowThreadProcessId(window, Some(&mut process_id));
    if process_id == 0 || process_id == std::process::id() {
        return BOOL(1);
    }
    let identity = identity_for(window, resolve_real_process(window, process_id));
    if !identity.process_name.is_empty() {
        apps.push(identity);
    }
    BOOL(1)
}

/// 枚举当前可切换的顶层窗口，供「按软件配置规则」的下拉框选择。
/// 同一个进程可能开多个窗口，这里按进程名去重，保留第一个窗口标题作为辨识线索。
pub(crate) fn list_running_apps() -> Vec<AppIdentity> {
    let mut apps: Vec<AppIdentity> = Vec::new();
    let _ = unsafe {
        EnumWindows(
            Some(collect_running_app),
            LPARAM(&mut apps as *mut Vec<AppIdentity> as isize),
        )
    };
    let mut seen = std::collections::HashSet::new();
    apps.retain(|app| seen.insert(app.process_name.to_lowercase()));
    apps.sort_by(|a, b| a.app_name.to_lowercase().cmp(&b.app_name.to_lowercase()));
    apps
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
