use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use image::DynamicImage;
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
    EnumWindows, GetClassNameW, GetCursorPos, GetForegroundWindow, GetWindow,
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

pub(crate) struct PreparedOcrCapture {
    context: CapturedActiveAppContext,
    image: Option<DynamicImage>,
    ocr_provider: crate::providers::capabilities::OcrProvider,
    max_chars: usize,
}

impl PreparedOcrCapture {
    pub(crate) fn without_image(
        context: CapturedActiveAppContext,
        ocr_provider: crate::providers::capabilities::OcrProvider,
        max_chars: usize,
    ) -> Self {
        Self {
            context,
            image: None,
            ocr_provider,
            max_chars,
        }
    }

    pub(crate) fn has_image(&self) -> bool {
        self.image.is_some()
    }

    pub(crate) fn context_for_fallback(&self) -> CapturedActiveAppContext {
        self.context.clone()
    }

    pub(crate) fn into_context(self) -> CapturedActiveAppContext {
        self.context
    }
}

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
    let window = HWND(target.window_handle as *mut std::ffi::c_void);
    // 与软件规则走同一套 UWP 解析，否则黑名单和提示词里的应用名会是框架宿主，
    // 和规则匹配到的真实应用对不上。关联不到时继续用宿主信息保底——上下文捕获
    // 有窗口元信息就有价值，不像规则匹配那样必须拿到确切的应用。
    let process_id = resolve_real_process(window, target.process_id).unwrap_or(target.process_id);
    let identity = identity_for(process_id, window_title(target.window_handle));
    let mut context = CapturedActiveAppContext {
        capture_method: method,
        app_name: identity.app_name,
        process_name: identity.process_name,
        process_id,
        window_title: identity.window_title,
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

/// 听写开始阶段只做隐私检查与屏幕拷贝。返回后线程即可退出，截图留在会话句柄中；
/// 此函数绝不加载本地 OCR 模型，也不调用远程 OCR 供应商。
pub(crate) fn prepare_ocr_capture(
    target: ActivationTarget,
    blocked_apps: &[String],
    options: CaptureOptions,
    cancelled: &Arc<AtomicBool>,
) -> PreparedOcrCapture {
    let started = Instant::now();
    let mut context = baseline_context(target, blocked_apps, ActiveAppContextExtractionMethod::Ocr);
    let mut image = None;

    if cancelled.load(Ordering::Acquire) {
        context.status = CaptureStatus::TimedOut;
    } else if context.status == CaptureStatus::Blocked {
        crate::development_debug_log(
            "active-app-context",
            format_args!("OCR 截图准备已拦截：黑名单应用 {}", context.process_name),
        );
    } else if expired(options.deadline) {
        context.status = CaptureStatus::TimedOut;
    } else {
        match focused_target_is_password(target) {
            Ok(true) => {
                context
                    .diagnostics
                    .push("焦点位于受保护输入控件，已停止上下文读取。".into());
                context.status = CaptureStatus::Sensitive;
            }
            Ok(false) => match screen_capture::capture_window(
                target.window_handle,
                options.occluding_window_handle,
                options.max_capture_side_override,
            ) {
                Ok(captured) => {
                    context.screenshot_width = captured.image.width();
                    context.screenshot_height = captured.image.height();
                    context.screenshot_elapsed_ms = captured.elapsed_ms;
                    if cancelled.load(Ordering::Acquire) || expired(options.deadline) {
                        context.status = CaptureStatus::TimedOut;
                    } else {
                        context.status = CaptureStatus::Empty;
                        image = Some(captured.image);
                    }
                }
                Err(error) => {
                    context.diagnostics.push(error);
                    context.status =
                        if cancelled.load(Ordering::Acquire) || expired(options.deadline) {
                            CaptureStatus::TimedOut
                        } else {
                            CaptureStatus::Failed
                        };
                }
            },
            Err(error) => {
                context.diagnostics.push(error);
                context.status = if cancelled.load(Ordering::Acquire) || expired(options.deadline) {
                    CaptureStatus::TimedOut
                } else {
                    CaptureStatus::Failed
                };
            }
        }
    }
    context.elapsed_ms = started.elapsed().as_millis() as u64;
    crate::development_debug_log(
        "active-app-context",
        format_args!(
            "OCR 开始阶段准备结束：状态={:?}，截图 {}×{}（{} ms），未提交识别",
            context.status,
            context.screenshot_width,
            context.screenshot_height,
            context.screenshot_elapsed_ms,
        ),
    );
    PreparedOcrCapture {
        context,
        image,
        ocr_provider: options.ocr_provider,
        max_chars: options.max_chars,
    }
}

pub(crate) fn recognize_prepared_ocr(
    mut prepared: PreparedOcrCapture,
    deadline: Instant,
    cancelled: &Arc<AtomicBool>,
) -> CapturedActiveAppContext {
    let started = Instant::now();
    let Some(image) = prepared.image.take() else {
        return prepared.context;
    };
    if cancelled.load(Ordering::Acquire) || expired(deadline) {
        prepared.context.status = CaptureStatus::TimedOut;
        prepared
            .context
            .diagnostics
            .push("OCR 提交前已取消或到达截止时间。".into());
        return prepared.context;
    }

    let output = ocr::run_full_window(
        prepared.ocr_provider,
        image,
        deadline,
        Arc::clone(cancelled),
    );
    prepared.context.status = match output {
        Ok(output) => {
            if let Some(diagnostic) = output.diagnostic {
                prepared.context.diagnostics.push(diagnostic);
            }
            prepared.context.ocr_text = output.text;
            prepared.context.ocr_blocks = output.blocks;
            prepared.context.model_init_ms = output.model_init_ms;
            prepared.context.ocr_elapsed_ms = output.elapsed_ms;
            prepared.context.truncated |= output.truncated;
            if prepared.context.ocr_text.is_empty() {
                prepared
                    .context
                    .diagnostics
                    .push("整窗截图成功，但 OCR 没有识别到文字。".into());
                CaptureStatus::Empty
            } else {
                prepared.context.source = Some(ContextSource::Ocr);
                CaptureStatus::Captured
            }
        }
        Err(error) => {
            prepared.context.diagnostics.push(error);
            if cancelled.load(Ordering::Acquire) || expired(deadline) {
                CaptureStatus::TimedOut
            } else {
                CaptureStatus::Failed
            }
        }
    };
    enforce_total_budget(&mut prepared.context, prepared.max_chars);
    prepared.context.elapsed_ms = prepared
        .context
        .elapsed_ms
        .saturating_add(started.elapsed().as_millis() as u64);
    crate::development_debug_log(
        "active-app-context",
        format_args!(
            "延迟 OCR 结束：状态={:?}，OCR {} ms，总工作耗时 {} ms",
            prepared.context.status, prepared.context.ocr_elapsed_ms, prepared.context.elapsed_ms,
        ),
    );
    prepared.context
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
/// 按窗口类名识别宿主，而不是先取进程名再比对——类名是窗口自己的属性，读它不需要
/// 打开进程句柄，枚举几十个窗口时差别很明显。
const UWP_FRAME_WINDOW_CLASS: &str = "ApplicationFrameWindow";
const UWP_CORE_WINDOW_CLASS: &str = "Windows.UI.Core.CoreWindow";

fn class_name(window: HWND) -> String {
    let mut buffer = [0u16; 256];
    let copied = unsafe { GetClassNameW(window, &mut buffer) };
    if copied <= 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buffer[..copied as usize])
}

struct CoreWindowLookup {
    title: String,
    host_process_id: u32,
    process_id: Option<u32>,
}

unsafe extern "system" fn match_core_window(window: HWND, lparam: LPARAM) -> BOOL {
    let lookup = &mut *(lparam.0 as *mut CoreWindowLookup);
    if class_name(window) != UWP_CORE_WINDOW_CLASS {
        return BOOL(1);
    }
    if window_title(window.0 as isize).as_deref() != Some(lookup.title.as_str()) {
        return BOOL(1);
    }
    let mut process_id = 0u32;
    GetWindowThreadProcessId(window, Some(&mut process_id));
    if process_id != 0 && process_id != lookup.host_process_id {
        lookup.process_id = Some(process_id);
        return BOOL(0);
    }
    BOOL(1)
}

/// 找出框架窗口承载的真实应用进程。
///
/// 常见的两种做法在 Win10 19045 上实测都不成立：`EnumChildWindows` 找不到
/// `CoreWindow`（框架窗口的子树全部属于宿主进程），`GetPropW` 取
/// `ApplicationViewCoreWindow` 返回空。`CoreWindow` 实际是独立的顶层窗口，
/// 与框架窗口只有标题一致这一条可用线索，只能按标题关联。
fn resolve_uwp_process(window: HWND, host_process_id: u32) -> Option<u32> {
    let mut lookup = CoreWindowLookup {
        title: window_title(window.0 as isize)?,
        host_process_id,
        process_id: None,
    };
    let _ = unsafe {
        EnumWindows(
            Some(match_core_window),
            LPARAM(&mut lookup as *mut CoreWindowLookup as isize),
        )
    };
    lookup.process_id
}

/// 窗口对应的真实进程。非 UWP 窗口原样返回，不做任何跨进程调用。
/// 框架窗口关联不到真实应用时返回 `None`——那样的条目只会显示成
/// `ApplicationFrameHost`，既认不出是哪个应用，配了规则也无法命中。
fn resolve_real_process(window: HWND, process_id: u32) -> Option<u32> {
    if class_name(window) != UWP_FRAME_WINDOW_CLASS {
        return Some(process_id);
    }
    resolve_uwp_process(window, process_id)
}

/// 组装标识。`window_title` 由调用方传入：枚举路径上标题已经作为过滤条件读过一次，
/// 这里复用，不再重复调用。
fn identity_for(process_id: u32, window_title: Option<String>) -> AppIdentity {
    let process_name = process_name(process_id).unwrap_or_default();
    let app_name = Path::new(&process_name)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(&process_name)
        .to_string();
    AppIdentity {
        process_name,
        app_name,
        window_title,
    }
}

pub(crate) fn app_identity(target: ActivationTarget) -> Option<AppIdentity> {
    let window = HWND(target.window_handle as *mut std::ffi::c_void);
    let process_id = resolve_real_process(window, target.process_id)?;
    let identity = identity_for(process_id, window_title(target.window_handle));
    (!identity.process_name.is_empty()).then_some(identity)
}

struct RunningAppScan {
    apps: Vec<AppIdentity>,
    seen_processes: std::collections::HashSet<u32>,
}

unsafe extern "system" fn collect_running_app(window: HWND, lparam: LPARAM) -> BOOL {
    let scan = &mut *(lparam.0 as *mut RunningAppScan);
    // 过滤条件按代价从低到高排列，尽量在打开进程句柄之前就淘汰掉窗口。
    // 只保留用户能看见、能切换过去的：可见、非工具窗口、非附属窗口、有标题。
    if !IsWindowVisible(window).as_bool() {
        return BOOL(1);
    }
    let ex_style = GetWindowLongPtrW(window, GWL_EXSTYLE) as u32;
    if ex_style & WS_EX_TOOLWINDOW.0 != 0 {
        return BOOL(1);
    }
    if GetWindow(window, GW_OWNER)
        .map(|owner| !owner.0.is_null())
        .unwrap_or(false)
    {
        return BOOL(1);
    }
    let Some(title) = window_title(window.0 as isize) else {
        return BOOL(1);
    };
    let mut process_id = 0u32;
    GetWindowThreadProcessId(window, Some(&mut process_id));
    if process_id == 0 || process_id == std::process::id() {
        return BOOL(1);
    }
    // UWP 解析必须排在按 PID 去重之前，否则多个 UWP 应用会因为共用宿主 PID 被
    // 折叠成一个。关联不到真实应用的框架窗口直接丢弃：它承载的应用会以自己的
    // CoreWindow 顶层窗口另行出现在枚举里，留着只会多一条认不出的重复条目。
    let Some(process_id) = resolve_real_process(window, process_id) else {
        return BOOL(1);
    };
    // 同一进程常开多个窗口（浏览器、资源管理器），只处理第一个就够，
    // 后面的窗口直接跳过——这是省下重复 OpenProcess 的主要来源。
    if !scan.seen_processes.insert(process_id) {
        return BOOL(1);
    }
    let identity = identity_for(process_id, Some(title));
    if !identity.process_name.is_empty() {
        scan.apps.push(identity);
    }
    BOOL(1)
}

/// 枚举当前可切换的顶层窗口，供软件规则的下拉框选择。
/// 同一个进程可能开多个窗口，按进程去重，保留第一个窗口标题作为辨识线索。
pub(crate) fn list_running_apps() -> Vec<AppIdentity> {
    let mut scan = RunningAppScan {
        apps: Vec::new(),
        seen_processes: std::collections::HashSet::new(),
    };
    let _ = unsafe {
        EnumWindows(
            Some(collect_running_app),
            LPARAM(&mut scan as *mut RunningAppScan as isize),
        )
    };
    let mut apps = scan.apps;
    // 同名不同 PID 的多实例（多开的同一软件）对规则来说是同一个软件，再去重一次。
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

    #[test]
    fn prepared_sensitive_result_never_enters_ocr() {
        let prepared = PreparedOcrCapture::without_image(
            CapturedActiveAppContext {
                status: CaptureStatus::Sensitive,
                app_name: "password-manager".into(),
                ..Default::default()
            },
            crate::providers::capabilities::OcrProvider::System,
            3_000,
        );
        let cancelled = Arc::new(AtomicBool::new(false));

        let result = recognize_prepared_ocr(
            prepared,
            Instant::now() + ActiveAppContextExtractionMethod::Ocr.timeout(),
            &cancelled,
        );

        assert_eq!(result.status, CaptureStatus::Sensitive);
        assert!(result.ocr_text.is_empty());
        assert!(result.format_for_prompt().is_empty());
    }
}
