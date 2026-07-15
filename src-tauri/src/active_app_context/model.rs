use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

pub(crate) const NATIVE_TEXT_CAPTURE_TIMEOUT: Duration = Duration::from_millis(800);
pub(crate) const OCR_CAPTURE_TIMEOUT: Duration = Duration::from_secs(5);
pub(crate) const DICTATION_RESOLVE_WAIT: Duration = Duration::from_millis(150);
pub(crate) const DEFAULT_MAX_CHARS: usize = 3_000;
pub(crate) const ABSOLUTE_MAX_CHARS: usize = 6_000;
pub(crate) const SUMMARY_PREVIEW_CHARS: usize = 500;

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

#[derive(Clone, Copy, Debug)]
pub(crate) struct ActivationTarget {
    pub(crate) window_handle: isize,
    pub(crate) process_id: u32,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct CaptureOptions {
    pub(crate) deadline: Instant,
    pub(crate) max_chars: usize,
    pub(crate) debug: bool,
    pub(crate) occluding_window_handle: Option<isize>,
    pub(crate) method: ActiveAppContextExtractionMethod,
}

impl CaptureOptions {
    pub(crate) fn for_method(method: ActiveAppContextExtractionMethod) -> Self {
        Self {
            deadline: Instant::now() + method.timeout(),
            max_chars: DEFAULT_MAX_CHARS,
            debug: false,
            occluding_window_handle: None,
            method,
        }
    }
}

impl Default for CaptureOptions {
    fn default() -> Self {
        Self::for_method(ActiveAppContextExtractionMethod::default())
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

#[derive(Clone, Copy, Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NormalizedRegion {
    pub(crate) left: f32,
    pub(crate) top: f32,
    pub(crate) right: f32,
    pub(crate) bottom: f32,
}

impl NormalizedRegion {
    pub(crate) fn clamped(self) -> Self {
        let left = self.left.clamp(0.0, 1.0);
        let top = self.top.clamp(0.0, 1.0);
        let right = self.right.clamp(left, 1.0);
        let bottom = self.bottom.clamp(top, 1.0);
        Self {
            left,
            top,
            right,
            bottom,
        }
    }
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OcrTextBlock {
    pub(crate) text: String,
    pub(crate) confidence: f32,
    pub(crate) bounds: NormalizedRegion,
}

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
