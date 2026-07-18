use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

pub(crate) const NATIVE_TEXT_CAPTURE_TIMEOUT: Duration = Duration::from_millis(800);
pub(crate) const OCR_CAPTURE_TIMEOUT: Duration = Duration::from_secs(5);
pub(crate) const DICTATION_RESOLVE_WAIT: Duration = Duration::from_millis(150);
pub(crate) const DEFAULT_MAX_CHARS: usize = 3_000;
pub(crate) const ABSOLUTE_MAX_CHARS: usize = 6_000;
pub(crate) const SUMMARY_PREVIEW_CHARS: usize = 500;
/// 窗口截图长边像素上限的默认值；调试窗口可临时覆盖，见 `CaptureOptions::max_capture_side_override`。
pub(crate) const DEFAULT_MAX_CAPTURE_SIDE: u32 = 1_600;

#[derive(Clone, Copy, Debug, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ActiveAppContextExtractionMethod {
    #[default]
    NativeText,
    Ocr,
}

impl<'de> Deserialize<'de> for ActiveAppContextExtractionMethod {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        Ok(match value.as_str() {
            Some("ocr") => Self::Ocr,
            _ => Self::NativeText,
        })
    }
}

impl ActiveAppContextExtractionMethod {
    pub(crate) fn timeout(self) -> Duration {
        match self {
            Self::NativeText => NATIVE_TEXT_CAPTURE_TIMEOUT,
            Self::Ocr => OCR_CAPTURE_TIMEOUT,
        }
    }
}

/// 仅在 `ActiveAppContextExtractionMethod::Ocr` 下生效：选择具体的窗口 OCR 实现。
#[derive(Clone, Copy, Debug, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum OcrEngineKind {
    #[default]
    System,
    PpOcr,
}

impl<'de> Deserialize<'de> for OcrEngineKind {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        Ok(match value.as_str() {
            Some("ppocr") => Self::PpOcr,
            _ => Self::System,
        })
    }
}

impl OcrEngineKind {
    /// 与自定义 `Deserialize` 使用同一套小写取值，供调试面板等场景直接输出给前端。
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::PpOcr => "ppocr",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ActivationTarget {
    pub(crate) window_handle: isize,
    pub(crate) process_id: u32,
    /// 全局快捷键触发时鼠标仍通常停在刚点击的输入区。该坐标仅在本次捕获中用于
    /// 定点定位辅助功能元素，不会持久化或发送到模型。
    pub(crate) cursor_position: Option<(i32, i32)>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct CaptureOptions {
    pub(crate) deadline: Instant,
    pub(crate) max_chars: usize,
    pub(crate) debug: bool,
    pub(crate) occluding_window_handle: Option<isize>,
    pub(crate) method: ActiveAppContextExtractionMethod,
    pub(crate) ocr_engine: OcrEngineKind,
    /// 仅调试窗口会设置：覆盖截图长边像素上限（默认见 `screen_capture::MAX_CAPTURE_SIDE`）。
    pub(crate) max_capture_side_override: Option<u32>,
}

impl CaptureOptions {
    pub(crate) fn for_method(
        method: ActiveAppContextExtractionMethod,
        ocr_engine: OcrEngineKind,
    ) -> Self {
        Self {
            deadline: Instant::now() + method.timeout(),
            max_chars: DEFAULT_MAX_CHARS,
            debug: false,
            occluding_window_handle: None,
            method,
            ocr_engine,
            max_capture_side_override: None,
        }
    }
}

impl Default for CaptureOptions {
    fn default() -> Self {
        Self::for_method(
            ActiveAppContextExtractionMethod::default(),
            OcrEngineKind::default(),
        )
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum CaptureStatus {
    Captured,
    #[default]
    Empty,
    Blocked,
    Sensitive,
    TimedOut,
    #[cfg_attr(windows, allow(dead_code))]
    Unsupported,
    Failed,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ContextSource {
    Ia2Text,
    UiaTextPattern,
    Win32Message,
    OfficeNative,
    Msaa,
    ClipboardDeep,
    Ocr,
}

// OCR 数据结构上提到 `crate::ocr`（providers 与本模块共用）；这里保留原路径的再导出。
pub(crate) use crate::ocr::{NormalizedRegion, OcrTextBlock};

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CapturedActiveAppContext {
    pub(crate) status: CaptureStatus,
    pub(crate) capture_method: ActiveAppContextExtractionMethod,
    pub(crate) source: Option<ContextSource>,
    pub(crate) app_name: String,
    pub(crate) process_name: String,
    pub(crate) process_id: u32,
    pub(crate) window_title: Option<String>,
    pub(crate) selected_text: Option<String>,
    pub(crate) focused_text: Option<String>,
    pub(crate) caret_context: Option<String>,
    pub(crate) visible_text: Vec<String>,
    pub(crate) document_text: Vec<String>,
    pub(crate) ocr_text: Vec<String>,
    pub(crate) ocr_blocks: Vec<OcrTextBlock>,
    pub(crate) screenshot_width: u32,
    pub(crate) screenshot_height: u32,
    pub(crate) screenshot_elapsed_ms: u64,
    pub(crate) model_init_ms: u64,
    pub(crate) ocr_elapsed_ms: u64,
    pub(crate) screenshot_data_url: Option<String>,
    pub(crate) diagnostics: Vec<String>,
    pub(crate) elapsed_ms: u64,
    pub(crate) truncated: bool,
}

impl CapturedActiveAppContext {
    pub(crate) fn with_status(status: CaptureStatus) -> Self {
        Self {
            status,
            ..Self::default()
        }
    }

    pub(crate) fn format_for_prompt(&self) -> String {
        if self.status != CaptureStatus::Captured {
            return String::new();
        }
        let mut lines = Vec::new();
        if !self.app_name.is_empty() {
            lines.push(format!("应用：{}", self.app_name));
        }
        if let Some(title) = self
            .window_title
            .as_deref()
            .filter(|value| !value.is_empty())
        {
            lines.push(format!("窗口：{title}"));
        }
        if let Some(value) = self
            .selected_text
            .as_deref()
            .filter(|value| !value.is_empty())
        {
            lines.push(format!("选中文本：{value}"));
        }
        if let Some(value) = self
            .focused_text
            .as_deref()
            .filter(|value| !value.is_empty())
        {
            lines.push(format!("当前输入区域：{value}"));
        }
        if let Some(value) = self
            .caret_context
            .as_deref()
            .filter(|value| !value.is_empty())
        {
            lines.push(format!("光标附近内容：{value}"));
        }
        if !self.visible_text.is_empty() {
            lines.push(format!("当前可见文字：{}", self.visible_text.join("\n")));
        }
        if !self.document_text.is_empty() {
            lines.push(format!("当前文档文字：{}", self.document_text.join("\n")));
        }
        if !self.ocr_text.is_empty() {
            lines.push(format!("窗口可见文字：{}", self.ocr_text.join("\n")));
        }
        lines.join("\n")
    }

    pub(crate) fn has_text_content(&self) -> bool {
        self.selected_text
            .as_deref()
            .is_some_and(|value| !value.is_empty())
            || self
                .focused_text
                .as_deref()
                .is_some_and(|value| !value.is_empty())
            || self
                .caret_context
                .as_deref()
                .is_some_and(|value| !value.is_empty())
            || !self.visible_text.is_empty()
            || !self.document_text.is_empty()
            || !self.ocr_text.is_empty()
    }

    /// 窗口元信息由本进程直接读取，不能让跨进程正文读取失败时一并丢失。
    /// 这也让智能整理至少能获知用户正在使用的应用和窗口场景。
    pub(crate) fn has_prompt_metadata(&self) -> bool {
        !self.app_name.trim().is_empty()
            || self
                .window_title
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty())
    }

    pub(crate) fn use_metadata_fallback(&mut self, reason: impl Into<String>) -> bool {
        if matches!(self.status, CaptureStatus::Blocked | CaptureStatus::Sensitive)
            || !self.has_prompt_metadata()
        {
            return false;
        }
        self.status = CaptureStatus::Captured;
        self.diagnostics.push(reason.into());
        true
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ActiveAppContextSummary {
    pub(crate) status: CaptureStatus,
    pub(crate) capture_method: ActiveAppContextExtractionMethod,
    pub(crate) source: Option<ContextSource>,
    pub(crate) app_name: String,
    pub(crate) process_name: String,
    pub(crate) window_title: Option<String>,
    pub(crate) preview: String,
    pub(crate) elapsed_ms: u64,
    pub(crate) truncated: bool,
}

impl From<&CapturedActiveAppContext> for ActiveAppContextSummary {
    fn from(value: &CapturedActiveAppContext) -> Self {
        Self {
            status: value.status,
            capture_method: value.capture_method,
            source: value.source,
            app_name: value.app_name.clone(),
            process_name: value.process_name.clone(),
            window_title: value.window_title.clone(),
            preview: super::normalize::truncate_chars(
                &value.format_for_prompt(),
                SUMMARY_PREVIEW_CHARS,
            )
            .0,
            elapsed_ms: value.elapsed_ms,
            truncated: value.truncated,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_method_defaults_to_native_text() {
        #[derive(Deserialize)]
        #[serde(default)]
        struct Wrapper {
            method: ActiveAppContextExtractionMethod,
        }
        impl Default for Wrapper {
            fn default() -> Self {
                Self {
                    method: ActiveAppContextExtractionMethod::default(),
                }
            }
        }
        let value: Wrapper = serde_json::from_str("{}").unwrap();
        assert_eq!(value.method, ActiveAppContextExtractionMethod::NativeText);
    }

    #[test]
    fn extraction_method_serializes_both_values_and_rejects_unknown_to_default() {
        assert_eq!(
            serde_json::to_string(&ActiveAppContextExtractionMethod::NativeText).unwrap(),
            "\"nativeText\""
        );
        assert_eq!(
            serde_json::to_string(&ActiveAppContextExtractionMethod::Ocr).unwrap(),
            "\"ocr\""
        );
        assert_eq!(
            serde_json::from_str::<ActiveAppContextExtractionMethod>("\"futureMethod\"").unwrap(),
            ActiveAppContextExtractionMethod::NativeText
        );
        assert_eq!(
            serde_json::from_str::<ActiveAppContextExtractionMethod>("123").unwrap(),
            ActiveAppContextExtractionMethod::NativeText
        );
    }

    #[test]
    fn summary_never_exposes_more_than_five_hundred_characters() {
        let context = CapturedActiveAppContext {
            status: CaptureStatus::Captured,
            document_text: vec!["内容".repeat(400)],
            ..Default::default()
        };
        let summary = ActiveAppContextSummary::from(&context);
        assert!(summary.preview.chars().count() <= SUMMARY_PREVIEW_CHARS);
    }

    #[test]
    fn failed_context_never_enters_the_prompt() {
        for status in [
            CaptureStatus::Empty,
            CaptureStatus::Blocked,
            CaptureStatus::Sensitive,
            CaptureStatus::TimedOut,
            CaptureStatus::Unsupported,
            CaptureStatus::Failed,
        ] {
            let context = CapturedActiveAppContext {
                status,
                document_text: vec!["不应发送".into()],
                ..Default::default()
            };
            assert!(context.format_for_prompt().is_empty());
        }
    }

    #[test]
    fn prompt_uses_fixed_source_priority() {
        let context = CapturedActiveAppContext {
            status: CaptureStatus::Captured,
            app_name: "Code".into(),
            window_title: Some("main.rs".into()),
            selected_text: Some("选区".into()),
            focused_text: Some("输入".into()),
            caret_context: Some("光标".into()),
            visible_text: vec!["可见".into()],
            document_text: vec!["文档".into()],
            ocr_text: vec!["OCR".into()],
            ..Default::default()
        };
        assert_eq!(
            context.format_for_prompt(),
            "应用：Code\n窗口：main.rs\n选中文本：选区\n当前输入区域：输入\n光标附近内容：光标\n当前可见文字：可见\n当前文档文字：文档\n窗口可见文字：OCR"
        );
    }

    #[test]
    fn timed_out_text_capture_can_safely_keep_window_metadata() {
        let mut context = CapturedActiveAppContext {
            status: CaptureStatus::TimedOut,
            app_name: "msedge".into(),
            window_title: Some("文档 - Microsoft Edge".into()),
            ..Default::default()
        };

        assert!(context.use_metadata_fallback("正文读取超时，仅使用基础窗口信息。"));
        assert_eq!(context.status, CaptureStatus::Captured);
        assert_eq!(
            context.format_for_prompt(),
            "应用：msedge\n窗口：文档 - Microsoft Edge"
        );
    }

    #[test]
    fn sensitive_capture_never_uses_metadata_fallback() {
        let mut context = CapturedActiveAppContext {
            status: CaptureStatus::Sensitive,
            app_name: "password-manager".into(),
            ..Default::default()
        };
        assert!(!context.use_metadata_fallback("不应使用"));
        assert!(context.format_for_prompt().is_empty());
    }

    #[test]
    fn runtime_summary_never_serializes_image_or_ocr_boxes() {
        let context = CapturedActiveAppContext {
            status: CaptureStatus::Captured,
            screenshot_data_url: Some("data:image/png;base64,secret".into()),
            ocr_blocks: vec![OcrTextBlock {
                text: "secret".into(),
                confidence: 1.0,
                bounds: NormalizedRegion::default(),
            }],
            ..Default::default()
        };
        let value = serde_json::to_value(ActiveAppContextSummary::from(&context)).unwrap();
        assert!(value.get("screenshotDataUrl").is_none());
        assert!(value.get("ocrBlocks").is_none());
    }
}
