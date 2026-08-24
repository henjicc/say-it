use serde::Deserialize;
use std::ffi::{c_char, c_void, CStr, CString};
use std::path::Path;
use std::ptr;

use crate::ocr::{NormalizedRegion, OcrTextBlock};

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MacWindowInfo {
    pub(crate) window_id: u32,
    pub(crate) process_id: u32,
    pub(crate) process_name: String,
    pub(crate) app_name: String,
    pub(crate) window_title: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MacApplicationIdentity {
    pub(crate) process_name: String,
    pub(crate) app_name: String,
}

pub(crate) struct MacWindowCapture {
    pub(crate) png: Vec<u8>,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MacAccessibilityBounds {
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) width: f64,
    pub(crate) height: f64,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MacAccessibilityContext {
    #[serde(default)]
    pub(crate) secure: bool,
    #[serde(default)]
    pub(crate) selected_text: Option<String>,
    #[serde(default)]
    pub(crate) selection_bounds: Option<MacAccessibilityBounds>,
    #[serde(default)]
    pub(crate) selection_editable: Option<bool>,
    #[serde(default)]
    pub(crate) focused_text: Option<String>,
    #[serde(default)]
    pub(crate) caret_context: Option<String>,
}

#[repr(C)]
struct NativeByteBuffer {
    data: *mut u8,
    length: usize,
    width: u32,
    height: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MacContextOcrPermissions {
    pub(crate) accessibility: bool,
    pub(crate) screen_recording: bool,
}

#[derive(Deserialize)]
struct NativeOcrBlock {
    text: String,
    confidence: f32,
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
}

pub(crate) type CapsLockCallback = unsafe extern "C" fn(*mut c_void, u64) -> bool;
pub(crate) type FnKeyCallback = unsafe extern "C" fn(*mut c_void, bool, u64) -> bool;
pub(crate) type EscapeCallback = unsafe extern "C" fn(*mut c_void, bool) -> bool;
pub(crate) type AudioCallback = unsafe extern "C" fn(*mut c_void, *const f32, usize);
pub(crate) type AudioErrorCallback = unsafe extern "C" fn(*mut c_void, *const c_char);
pub(crate) type MouseMonitorCallback =
    unsafe extern "C" fn(*mut c_void, f64, f64, bool, bool, bool);

unsafe extern "C" {
    fn sayit_macos_free_string(value: *mut c_char);
    fn sayit_macos_free_bytes(value: *mut u8);
    fn sayit_macos_decode_audio_file(
        path: *const c_char,
        samples: *mut *mut f32,
        count: *mut usize,
        error: *mut *mut c_char,
    ) -> bool;
    fn sayit_macos_context_ocr_permissions(request: bool) -> u32;
    fn sayit_macos_accessibility_permission(request: bool) -> bool;
    fn sayit_macos_system_fonts_json(error: *mut *mut c_char) -> *mut c_char;
    fn sayit_macos_volume_available_capacity(
        path: *const c_char,
        capacity: *mut u64,
        error: *mut *mut c_char,
    ) -> bool;
    fn sayit_macos_frontmost_window_json(error: *mut *mut c_char) -> *mut c_char;
    fn sayit_macos_activate_application(process_id: u32, error: *mut *mut c_char) -> bool;
    fn sayit_macos_window_json(
        window_id: u32,
        process_id: u32,
        error: *mut *mut c_char,
    ) -> *mut c_char;
    fn sayit_macos_running_apps_json(error: *mut *mut c_char) -> *mut c_char;
    fn sayit_macos_application_bundle_json(
        path: *const c_char,
        error: *mut *mut c_char,
    ) -> *mut c_char;
    fn sayit_macos_focused_input_security(process_id: u32, error: *mut *mut c_char) -> i32;
    fn sayit_macos_focused_input_editable(process_id: u32, error: *mut *mut c_char) -> i32;
    fn sayit_macos_copy_selection_text(process_id: u32, error: *mut *mut c_char) -> *mut c_char;
    fn sayit_macos_accessibility_context_json(
        process_id: u32,
        max_chars: u32,
        error: *mut *mut c_char,
    ) -> *mut c_char;
    fn sayit_macos_capture_window_png(
        window_id: u32,
        max_side: u32,
        output: *mut NativeByteBuffer,
        error: *mut *mut c_char,
    ) -> bool;
    fn sayit_macos_vision_ocr_png(
        bytes: *const u8,
        length: usize,
        error: *mut *mut c_char,
    ) -> *mut c_char;
    fn sayit_macos_keyboard_tap_start(
        caps_lock_callback: CapsLockCallback,
        fn_key_callback: FnKeyCallback,
        escape_callback: EscapeCallback,
        context: *mut c_void,
        monitor_caps_lock: bool,
        monitor_fn_key: bool,
        monitor_escape: bool,
        error: *mut *mut c_char,
    ) -> *mut c_void;
    fn sayit_macos_keyboard_tap_stop(handle: *mut c_void);
    fn sayit_macos_mouse_monitor_start(
        callback: MouseMonitorCallback,
        context: *mut c_void,
        error: *mut *mut c_char,
    ) -> *mut c_void;
    fn sayit_macos_mouse_monitor_stop(handle: *mut c_void);
    fn sayit_macos_system_audio_start(
        callback: AudioCallback,
        error_callback: AudioErrorCallback,
        context: *mut c_void,
        error: *mut *mut c_char,
    ) -> *mut c_void;
    fn sayit_macos_system_audio_stop(handle: *mut c_void);
    fn sayit_macos_place_indicator_window(
        ns_window: *mut c_void,
        width: f64,
        height: f64,
        anchor: i32,
        offset_y: f64,
        error: *mut *mut c_char,
    ) -> bool;
    fn sayit_macos_indicator_visible_screen_size(
        ns_window: *mut c_void,
        width: *mut f64,
        height: *mut f64,
        error: *mut *mut c_char,
    ) -> bool;
    fn sayit_macos_configure_floating_orb_window(
        ns_window: *mut c_void,
        nonactivating: bool,
        error: *mut *mut c_char,
    ) -> bool;
    fn sayit_macos_floating_orb_owns_pointer_event(ns_window: *mut c_void) -> bool;
    fn sayit_macos_paste_current_clipboard(error: *mut *mut c_char) -> bool;
    fn sayit_macos_paste_text(text: *const c_char, error: *mut *mut c_char) -> bool;
    fn sayit_macos_type_text(text: *const c_char, error: *mut *mut c_char) -> bool;
    fn sayit_macos_press_return(process_id: u32, error: *mut *mut c_char) -> bool;
}

unsafe fn take_string(value: *mut c_char) -> Option<String> {
    if value.is_null() {
        return None;
    }
    let result = CStr::from_ptr(value).to_string_lossy().into_owned();
    sayit_macos_free_string(value);
    Some(result)
}

unsafe fn native_result(value: *mut c_char, error: *mut c_char) -> Result<String, String> {
    if let Some(value) = take_string(value) {
        if !error.is_null() {
            let _ = take_string(error);
        }
        Ok(value)
    } else {
        Err(take_string(error).unwrap_or_else(|| "macOS 原生能力调用失败".into()))
    }
}

pub(crate) fn frontmost_window() -> Result<MacWindowInfo, String> {
    let mut error = ptr::null_mut();
    let value = unsafe { sayit_macos_frontmost_window_json(&mut error) };
    let json = unsafe { native_result(value, error)? };
    serde_json::from_str(&json).map_err(|error| format!("解析 macOS 前台窗口失败：{error}"))
}

pub(crate) fn activate_application(process_id: u32) -> Result<(), String> {
    let mut error = ptr::null_mut();
    if unsafe { sayit_macos_activate_application(process_id, &mut error) } {
        Ok(())
    } else {
        Err(unsafe {
            take_string(error).unwrap_or_else(|| "无法重新激活听写开始时的应用".into())
        })
    }
}

pub(crate) fn running_apps() -> Result<Vec<MacWindowInfo>, String> {
    let mut error = ptr::null_mut();
    let value = unsafe { sayit_macos_running_apps_json(&mut error) };
    let json = unsafe { native_result(value, error)? };
    serde_json::from_str(&json).map_err(|error| format!("解析 macOS 运行中应用失败：{error}"))
}

pub(crate) fn window_info(window_id: u32, process_id: u32) -> Result<MacWindowInfo, String> {
    let mut error = ptr::null_mut();
    let value = unsafe { sayit_macos_window_json(window_id, process_id, &mut error) };
    let json = unsafe { native_result(value, error)? };
    serde_json::from_str(&json).map_err(|error| format!("解析 macOS 窗口信息失败：{error}"))
}

pub(crate) fn application_bundle(path: &Path) -> Result<MacApplicationIdentity, String> {
    let path = CString::new(path.to_string_lossy().as_bytes())
        .map_err(|_| "macOS 应用路径包含空字符".to_string())?;
    let mut error = ptr::null_mut();
    let value = unsafe { sayit_macos_application_bundle_json(path.as_ptr(), &mut error) };
    let json = unsafe { native_result(value, error)? };
    serde_json::from_str(&json).map_err(|error| format!("解析 macOS 应用信息失败：{error}"))
}

pub(crate) fn focused_input_is_secure(process_id: u32) -> Result<bool, String> {
    let mut error = ptr::null_mut();
    let result = unsafe { sayit_macos_focused_input_security(process_id, &mut error) };
    match result {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(unsafe {
            take_string(error).unwrap_or_else(|| "无法确认当前输入区域安全性".into())
        }),
    }
}

pub(crate) fn focused_input_is_editable(process_id: u32) -> Result<bool, String> {
    let mut error = ptr::null_mut();
    let result = unsafe { sayit_macos_focused_input_editable(process_id, &mut error) };
    match result {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(unsafe {
            take_string(error).unwrap_or_else(|| "无法确认当前焦点控件是否可编辑".into())
        }),
    }
}

pub(crate) fn copy_selection_text(process_id: u32) -> Result<String, String> {
    let mut error = std::ptr::null_mut();
    let value = unsafe { sayit_macos_copy_selection_text(process_id, &mut error) };
    unsafe { native_result(value, error) }
}

pub(crate) fn accessibility_context(
    process_id: u32,
    max_chars: usize,
) -> Result<MacAccessibilityContext, String> {
    let mut error = ptr::null_mut();
    let value = unsafe {
        sayit_macos_accessibility_context_json(
            process_id,
            max_chars.min(u32::MAX as usize) as u32,
            &mut error,
        )
    };
    let json = unsafe { native_result(value, error)? };
    parse_accessibility_context(&json)
}

fn parse_accessibility_context(json: &str) -> Result<MacAccessibilityContext, String> {
    serde_json::from_str(json).map_err(|error| format!("解析 macOS 辅助功能文本失败：{error}"))
}

pub(crate) fn context_ocr_permissions(request: bool) -> MacContextOcrPermissions {
    let bits = unsafe { sayit_macos_context_ocr_permissions(request) };
    MacContextOcrPermissions {
        accessibility: bits & (1 << 0) != 0,
        screen_recording: bits & (1 << 1) != 0,
    }
}

pub(crate) fn prepare_context_ocr_permissions(request: bool) -> Result<(), String> {
    let permissions = context_ocr_permissions(request);
    if permissions.accessibility && permissions.screen_recording {
        return Ok(());
    }

    let mut missing = Vec::new();
    if !permissions.accessibility {
        missing.push("辅助功能");
    }
    if !permissions.screen_recording {
        missing.push("屏幕录制");
    }
    let action = if request {
        "系统已尝试打开授权入口；请在系统设置 → 隐私与安全性中允许当前运行的说吧！进程"
    } else {
        "请在系统设置 → 隐私与安全性中允许当前运行的说吧！进程"
    };
    Err(format!(
        "macOS 窗口 OCR 需要{}权限。{}；授权后请完全退出并重新启动应用。开发态通常显示为 say-it 或终端启动的开发进程。",
        missing.join("、"),
        action
    ))
}

pub(crate) fn prepare_accessibility_permission(request: bool) -> Result<(), String> {
    if unsafe { sayit_macos_accessibility_permission(request) } {
        Ok(())
    } else {
        Err("macOS 文本提取需要辅助功能权限；请在系统设置 → 隐私与安全性 → 辅助功能中允许当前运行的说吧！进程，授权后完全退出并重新启动应用。".into())
    }
}

pub(crate) fn system_font_families() -> Result<Vec<String>, String> {
    let mut error = ptr::null_mut();
    let value = unsafe { sayit_macos_system_fonts_json(&mut error) };
    let json = unsafe { native_result(value, error)? };
    let mut families: Vec<String> = serde_json::from_str(&json)
        .map_err(|error| format!("解析 macOS 系统字体列表失败：{error}"))?;
    families.retain(|family| !family.trim().is_empty());
    families.sort_by_key(|family| family.to_lowercase());
    families.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
    Ok(families)
}

pub(crate) fn volume_available_capacity(path: &Path) -> Result<u64, String> {
    let path = CString::new(path.to_string_lossy().as_bytes())
        .map_err(|_| "macOS 数据目录路径包含空字符".to_string())?;
    let mut capacity = 0;
    let mut error = ptr::null_mut();
    if unsafe { sayit_macos_volume_available_capacity(path.as_ptr(), &mut capacity, &mut error) } {
        Ok(capacity)
    } else {
        Err(unsafe {
            take_string(error).unwrap_or_else(|| "读取 macOS 磁盘剩余空间失败".into())
        })
    }
}

pub(crate) fn decode_audio_file(path: &Path) -> Result<Vec<f32>, String> {
    let path = CString::new(path.to_string_lossy().as_bytes())
        .map_err(|_| "macOS 音频文件路径包含空字符".to_string())?;
    let mut samples = ptr::null_mut();
    let mut count = 0_usize;
    let mut error = ptr::null_mut();
    let success = unsafe {
        sayit_macos_decode_audio_file(path.as_ptr(), &mut samples, &mut count, &mut error)
    };
    if !success || samples.is_null() || count == 0 {
        if !samples.is_null() {
            unsafe { sayit_macos_free_bytes(samples.cast()) };
        }
        return Err(unsafe {
            take_string(error).unwrap_or_else(|| "macOS 原生音频解码失败".into())
        });
    }
    let output = unsafe { std::slice::from_raw_parts(samples, count).to_vec() };
    unsafe { sayit_macos_free_bytes(samples.cast()) };
    if !error.is_null() {
        let _ = unsafe { take_string(error) };
    }
    Ok(output)
}

pub(crate) fn capture_window(window_id: u32, max_side: u32) -> Result<MacWindowCapture, String> {
    let mut output = NativeByteBuffer {
        data: ptr::null_mut(),
        length: 0,
        width: 0,
        height: 0,
    };
    let mut error = ptr::null_mut();
    let success =
        unsafe { sayit_macos_capture_window_png(window_id, max_side, &mut output, &mut error) };
    if !success || output.data.is_null() || output.length == 0 {
        return Err(unsafe {
            take_string(error).unwrap_or_else(|| "macOS 窗口截图失败".into())
        });
    }
    let png = unsafe { std::slice::from_raw_parts(output.data, output.length).to_vec() };
    unsafe { sayit_macos_free_bytes(output.data) };
    Ok(MacWindowCapture {
        png,
        width: output.width,
        height: output.height,
    })
}

pub(crate) fn vision_ocr(png: &[u8]) -> Result<Vec<OcrTextBlock>, String> {
    let mut error = ptr::null_mut();
    let value = unsafe { sayit_macos_vision_ocr_png(png.as_ptr(), png.len(), &mut error) };
    let json = unsafe { native_result(value, error)? };
    let blocks: Vec<NativeOcrBlock> = serde_json::from_str(&json)
        .map_err(|error| format!("解析 macOS Vision OCR 结果失败：{error}"))?;
    Ok(blocks
        .into_iter()
        .filter_map(|block| {
            let text = crate::ocr::normalize_text(&block.text);
            if text.is_empty() {
                return None;
            }
            Some(OcrTextBlock {
                text,
                confidence: block.confidence.clamp(0.0, 1.0),
                bounds: NormalizedRegion {
                    left: block.left,
                    top: block.top,
                    right: block.right,
                    bottom: block.bottom,
                }
                .clamped(),
            })
        })
        .collect())
}

pub(crate) fn start_keyboard_tap(
    caps_lock_callback: CapsLockCallback,
    fn_key_callback: FnKeyCallback,
    escape_callback: EscapeCallback,
    monitor_caps_lock: bool,
    monitor_fn_key: bool,
    monitor_escape: bool,
) -> Result<usize, String> {
    let mut error = ptr::null_mut();
    let handle = unsafe {
        sayit_macos_keyboard_tap_start(
            caps_lock_callback,
            fn_key_callback,
            escape_callback,
            ptr::null_mut(),
            monitor_caps_lock,
            monitor_fn_key,
            monitor_escape,
            &mut error,
        )
    };
    if handle.is_null() {
        Err(unsafe {
            take_string(error).unwrap_or_else(|| "启动 macOS 键盘监听失败".into())
        })
    } else {
        Ok(handle as usize)
    }
}

pub(crate) fn stop_keyboard_tap(handle: usize) {
    if handle != 0 {
        unsafe { sayit_macos_keyboard_tap_stop(handle as *mut c_void) };
    }
}

pub(crate) fn start_mouse_monitor(callback: MouseMonitorCallback) -> Result<usize, String> {
    let mut error = std::ptr::null_mut();
    let handle = unsafe {
        sayit_macos_mouse_monitor_start(callback, std::ptr::null_mut(), &mut error)
    };
    if handle.is_null() {
        Err(unsafe { take_string(error) }
            .unwrap_or_else(|| "无法启动 macOS 鼠标手势监听".into()))
    } else {
        if !error.is_null() {
            let _ = unsafe { take_string(error) };
        }
        Ok(handle as usize)
    }
}

pub(crate) fn stop_mouse_monitor(handle: usize) {
    if handle != 0 {
        unsafe { sayit_macos_mouse_monitor_stop(handle as *mut c_void) };
    }
}

pub(crate) unsafe fn start_system_audio(
    callback: AudioCallback,
    error_callback: AudioErrorCallback,
    context: *mut c_void,
) -> Result<usize, String> {
    let mut error = ptr::null_mut();
    let handle = sayit_macos_system_audio_start(callback, error_callback, context, &mut error);
    if handle.is_null() {
        Err(take_string(error).unwrap_or_else(|| "启动 macOS 系统音频采集失败".into()))
    } else {
        Ok(handle as usize)
    }
}

pub(crate) fn stop_system_audio(handle: usize) {
    if handle != 0 {
        unsafe { sayit_macos_system_audio_stop(handle as *mut c_void) };
    }
}

pub(crate) fn place_indicator_window(
    ns_window: *mut c_void,
    width: f64,
    height: f64,
    anchor: &str,
    offset_y: f64,
) -> Result<(), String> {
    let anchor = match anchor {
        "top" => 0,
        "center" => 1,
        _ => 2,
    };
    let mut error = ptr::null_mut();
    let success = unsafe {
        sayit_macos_place_indicator_window(ns_window, width, height, anchor, offset_y, &mut error)
    };
    if success {
        Ok(())
    } else {
        Err(unsafe {
            take_string(error).unwrap_or_else(|| "定位 macOS 悬浮窗口失败".into())
        })
    }
}

pub(crate) fn indicator_visible_screen_size(ns_window: *mut c_void) -> Result<(f64, f64), String> {
    let mut width = 0.0;
    let mut height = 0.0;
    let mut error = ptr::null_mut();
    let success = unsafe {
        sayit_macos_indicator_visible_screen_size(ns_window, &mut width, &mut height, &mut error)
    };
    if success && width > 0.0 && height > 0.0 {
        Ok((width, height))
    } else {
        Err(unsafe {
            take_string(error).unwrap_or_else(|| "读取 macOS 可用屏幕区域失败".into())
        })
    }
}

pub(crate) fn configure_floating_orb_window(
    ns_window: *mut c_void,
    nonactivating: bool,
) -> Result<(), String> {
    let mut error = ptr::null_mut();
    if unsafe {
        sayit_macos_configure_floating_orb_window(ns_window, nonactivating, &mut error)
    } {
        Ok(())
    } else {
        Err(unsafe {
            take_string(error).unwrap_or_else(|| "配置 macOS 悬浮球窗口失败".into())
        })
    }
}

pub(crate) fn floating_orb_owns_pointer_event(ns_window: *mut c_void) -> bool {
    unsafe { sayit_macos_floating_orb_owns_pointer_event(ns_window) }
}

pub(crate) fn paste_text(text: &str) -> Result<(), String> {
    let text = CString::new(text).map_err(|_| "待粘贴文本包含空字符".to_string())?;
    let mut error = ptr::null_mut();
    if unsafe { sayit_macos_paste_text(text.as_ptr(), &mut error) } {
        Ok(())
    } else {
        Err(unsafe { take_string(error).unwrap_or_else(|| "执行 macOS 粘贴失败".into()) })
    }
}

pub(crate) fn paste_current_clipboard() -> Result<(), String> {
    let mut error = ptr::null_mut();
    if unsafe { sayit_macos_paste_current_clipboard(&mut error) } {
        Ok(())
    } else {
        Err(unsafe {
            take_string(error).unwrap_or_else(|| "发送 macOS 粘贴快捷键失败".into())
        })
    }
}

pub(crate) fn type_text(text: &str) -> Result<(), String> {
    let text = CString::new(text).map_err(|_| "待输入文本包含空字符".to_string())?;
    let mut error = ptr::null_mut();
    if unsafe { sayit_macos_type_text(text.as_ptr(), &mut error) } {
        Ok(())
    } else {
        Err(unsafe {
            take_string(error).unwrap_or_else(|| "执行 macOS 逐字输入失败".into())
        })
    }
}

pub(crate) fn press_return(process_id: u32) -> Result<(), String> {
    let mut error = std::ptr::null_mut();
    if unsafe { sayit_macos_press_return(process_id, &mut error) } {
        if !error.is_null() {
            let _ = unsafe { take_string(error) };
        }
        Ok(())
    } else {
        Err(unsafe { take_string(error) }
            .unwrap_or_else(|| "macOS 模拟回车失败".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accessibility_context_accepts_partial_native_payload() {
        let context = parse_accessibility_context(
            r#"{"selectedText":"选区","caretContext":"光标附近","selectionBounds":{"x":12.5,"y":20,"width":80,"height":24},"selectionEditable":true}"#,
        )
        .unwrap();
        assert_eq!(context.selected_text.as_deref(), Some("选区"));
        assert!(!context.secure);
        assert_eq!(context.focused_text, None);
        assert_eq!(context.caret_context.as_deref(), Some("光标附近"));
        assert_eq!(context.selection_editable, Some(true));
        assert_eq!(
            context.selection_bounds.as_ref().map(|value| value.x),
            Some(12.5)
        );

        let secure = parse_accessibility_context(r#"{"secure":true}"#).unwrap();
        assert!(secure.secure);
        assert_eq!(secure.selected_text, None);
    }

    #[test]
    fn application_bundle_resolves_system_application_identity() {
        let identity = application_bundle(Path::new("/System/Applications/TextEdit.app")).unwrap();
        assert!(!identity.process_name.trim().is_empty());
        assert!(!identity.app_name.trim().is_empty());
    }

    #[test]
    fn text_injection_rejects_embedded_null_before_native_call() {
        assert_eq!(type_text("前\0后").unwrap_err(), "待输入文本包含空字符");
        assert_eq!(paste_text("前\0后").unwrap_err(), "待粘贴文本包含空字符");
    }
}
