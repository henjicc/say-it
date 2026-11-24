//! 按屏幕坐标用 UI Automation 取光标下的控件和文字。动态上下文的鼠标指向先走这里，取不到再截图 OCR；
//! 开发者「指向测试」也用它。
use std::ffi::c_void;

use serde::Serialize;
use windows::Win32::Foundation::POINT;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, SAFEARRAY, CLSCTX_INPROC_SERVER,
    COINIT_MULTITHREADED,
};
use windows::Win32::System::Ole::{
    SafeArrayAccessData, SafeArrayDestroy, SafeArrayGetUBound, SafeArrayUnaccessData,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationTextPattern,
    IUIAutomationTextRange, TextPatternRangeEndpoint_End, TextPatternRangeEndpoint_Start,
    TextUnit_Line, UIA_TextPatternId,
};

/// 往上找能按坐标取文字（文本模式）的元素，最多找这么多层。
const TEXT_PATTERN_DEPTH: usize = 10;
const LINE_CHARS: i32 = 2_000;
const NAME_CHARS: usize = 400;
/// 不超过这么高的控件（两行字左右）才直接用它的名称：更高的多半是面板、文档或整个窗口。
const CONTROL_MAX_HEIGHT: i32 = 80;
/// 取到的那一行的边框要在光标这么多像素以内，才算光标真的指着这一行。
const LINE_TOLERANCE: f64 = 4.0;

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ElementInfo {
    pub(crate) control_type: String,
    pub(crate) name: String,
    pub(crate) class_name: String,
    /// [left, top, right, bottom]，屏幕物理像素。
    pub(crate) rect: [i32; 4],
}

impl ElementInfo {
    pub(crate) fn area(&self) -> i64 {
        (self.rect[2] - self.rect[0]).max(0) as i64 * (self.rect[3] - self.rect[1]).max(0) as i64
    }

    fn contains(&self, x: i32, y: i32) -> bool {
        self.rect[0] <= x && x < self.rect[2] && self.rect[1] <= y && y < self.rect[3]
    }
}

/// 指向的文字从哪一级取到的。
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum PointSource {
    /// 按坐标取到光标所在的整行原文。
    Text,
    /// 光标下小控件的名称。
    Control,
    /// 截图识别。
    Ocr,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LineAt {
    pub(crate) line: String,
    /// 光标落在这一行的第几个字。
    pub(crate) offset: usize,
    /// 提供文本模式的元素。
    pub(crate) source: ElementInfo,
}

/// 在当前线程初始化 COM（多线程套间），离开时配对释放。
pub(crate) struct ComScope(bool);

impl ComScope {
    pub(crate) fn enter() -> Self {
        Self(unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).is_ok() })
    }
}

impl Drop for ComScope {
    fn drop(&mut self) {
        if self.0 {
            unsafe { CoUninitialize() };
        }
    }
}

pub(crate) fn automation() -> Result<IUIAutomation, String> {
    unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }
        .map_err(|error| format!("无法初始化 UI Automation：{error}"))
}

pub(crate) fn info(element: &IUIAutomationElement) -> Result<ElementInfo, String> {
    unsafe {
        let rect = element.CurrentBoundingRectangle().map_err(|error| format!("读控件边框失败：{error}"))?;
        let text = |value: windows::core::Result<windows::core::BSTR>| -> String {
            value.map(|value| value.to_string().chars().take(NAME_CHARS).collect()).unwrap_or_default()
        };
        Ok(ElementInfo {
            control_type: text(element.CurrentLocalizedControlType()),
            name: text(element.CurrentName()),
            class_name: text(element.CurrentClassName()),
            rect: [rect.left, rect.top, rect.right, rect.bottom],
        })
    }
}

/// 光标下的元素；在本应用自己的窗口上时不取。
pub(crate) fn element_at(automation: &IUIAutomation, x: i32, y: i32) -> Result<IUIAutomationElement, String> {
    unsafe {
        let element = automation
            .ElementFromPoint(POINT { x, y })
            .map_err(|error| format!("ElementFromPoint 失败：{error}"))?;
        if element.CurrentProcessId().ok() == Some(std::process::id() as i32) {
            return Err("光标在说吧自己的窗口上".into());
        }
        Ok(element)
    }
}

/// 光标下的元素和它的祖先（不含桌面根），最多 `depth` 层祖先。
pub(crate) fn element_chain(automation: &IUIAutomation, x: i32, y: i32, depth: usize) -> Result<Vec<ElementInfo>, String> {
    unsafe {
        let element = element_at(automation, x, y)?;
        let walker = automation.ControlViewWalker().map_err(|error| format!("取控件树失败：{error}"))?;
        let root = automation.GetRootElement().map_err(|error| format!("取桌面根失败：{error}"))?;
        let mut chain = vec![info(&element)?];
        let mut current = element;
        while chain.len() <= depth {
            let Ok(parent) = walker.GetParentElement(&current) else { break };
            if automation.CompareElements(&parent, &root).is_ok_and(|same| same.as_bool()) {
                break;
            }
            chain.push(info(&parent)?);
            current = parent;
        }
        Ok(chain)
    }
}

/// 光标所在的整行原文和光标在行里的位置：从光标下的元素往上找第一个支持文本模式的元素，
/// 按坐标取文字范围。取到的那一行的边框没有罩住光标时（有的软件会返回别处的文字）作废。
pub(crate) fn line_at(automation: &IUIAutomation, x: i32, y: i32) -> Result<LineAt, String> {
    unsafe {
        let walker = automation.ControlViewWalker().map_err(|error| format!("取控件树失败：{error}"))?;
        let mut element = element_at(automation, x, y)?;
        for _ in 0..TEXT_PATTERN_DEPTH {
            if let Ok(pattern) = element.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId) {
                let at = pattern
                    .RangeFromPoint(POINT { x, y })
                    .map_err(|error| format!("RangeFromPoint 失败：{error}"))?;
                let line = at.Clone().map_err(|error| format!("复制文字范围失败：{error}"))?;
                line.ExpandToEnclosingUnit(TextUnit_Line)
                    .map_err(|error| format!("扩到整行失败：{error}"))?;
                if !covers(&line, x, y)? {
                    return Err("取到的那一行不在光标位置".into());
                }
                let before = line.Clone().map_err(|error| format!("复制文字范围失败：{error}"))?;
                before
                    .MoveEndpointByRange(TextPatternRangeEndpoint_End, &at, TextPatternRangeEndpoint_Start)
                    .map_err(|error| format!("定位光标所在的字失败：{error}"))?;
                let text = line.GetText(LINE_CHARS).map_err(|error| format!("读整行文字失败：{error}"))?.to_string();
                let before = before.GetText(LINE_CHARS).map(|value| value.to_string()).unwrap_or_default();
                let leading = text.chars().take_while(|c| c.is_whitespace()).count();
                let line = text.trim().to_string();
                if line.is_empty() {
                    return Err("光标所在的那一行没有文字".into());
                }
                let offset = before.chars().count().saturating_sub(leading).min(line.chars().count() - 1);
                return Ok(LineAt { line, offset, source: info(&element)? });
            }
            element = walker
                .GetParentElement(&element)
                .map_err(|_| "往上没有支持文本模式的元素".to_string())?;
        }
        Err(format!("往上 {TEXT_PATTERN_DEPTH} 层都没有支持文本模式的元素"))
    }
}

/// 文字范围的边框（可能分成好几块）有一块罩住了光标。
fn covers(range: &IUIAutomationTextRange, x: i32, y: i32) -> Result<bool, String> {
    let rects = unsafe { range.GetBoundingRectangles() }.map_err(|error| format!("读文字范围边框失败：{error}"))?;
    let values = unsafe { safe_array_f64(rects) }?;
    let (x, y) = (x as f64, y as f64);
    Ok(values.chunks_exact(4).any(|rect| {
        x >= rect[0] - LINE_TOLERANCE
            && x <= rect[0] + rect[2] + LINE_TOLERANCE
            && y >= rect[1] - LINE_TOLERANCE
            && y <= rect[1] + rect[3] + LINE_TOLERANCE
    }))
}

/// 读出一维 double 数组并释放它。
unsafe fn safe_array_f64(array: *mut SAFEARRAY) -> Result<Vec<f64>, String> {
    if array.is_null() {
        return Ok(Vec::new());
    }
    let upper = SafeArrayGetUBound(array, 1).unwrap_or(-1);
    let mut data: *mut c_void = std::ptr::null_mut();
    let values = match SafeArrayAccessData(array, &mut data) {
        Ok(()) if upper >= 0 && !data.is_null() => {
            let values = std::slice::from_raw_parts(data.cast::<f64>(), (upper + 1) as usize).to_vec();
            let _ = SafeArrayUnaccessData(array);
            values
        }
        Ok(()) => {
            let _ = SafeArrayUnaccessData(array);
            Vec::new()
        }
        Err(error) => {
            let _ = SafeArrayDestroy(array);
            return Err(format!("读文字范围边框失败：{error}"));
        }
    };
    let _ = SafeArrayDestroy(array);
    Ok(values)
}

/// 前两级：按坐标取整行再截光标所在的一小段；不行就用光标下小控件的名称。都不行时返回 `None`，
/// 同时给出光标下的元素，调用方截图 OCR 时用它限制截取范围。
pub(crate) fn pointed_by_uia(x: i32, y: i32) -> (Option<(String, PointSource)>, Option<ElementInfo>) {
    let _com = ComScope::enter();
    let Ok(automation) = automation() else { return (None, None) };
    let leaf = element_at(&automation, x, y).ok().and_then(|element| info(&element).ok());
    if let Ok(line) = line_at(&automation, x, y) {
        if let Some(phrase) = super::dynamic::phrase_around(&line.line, line.offset) {
            return (Some((phrase, PointSource::Text)), leaf);
        }
    }
    let control = leaf.as_ref().filter(|leaf| {
        leaf.contains(x, y) && leaf.rect[3] - leaf.rect[1] <= CONTROL_MAX_HEIGHT && !leaf.name.trim().is_empty()
    });
    if let Some(leaf) = control {
        let width = (leaf.rect[2] - leaf.rect[0]).max(1) as f32;
        let ratio = (x - leaf.rect[0]) as f32 / width;
        if let Some(phrase) = super::dynamic::phrase_at(leaf.name.trim(), ratio) {
            return (Some((phrase, PointSource::Control)), Some(leaf.clone()));
        }
    }
    (None, leaf)
}
