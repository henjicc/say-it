use serde::Serialize;
use std::time::{Duration, Instant};

pub(crate) const CAPTURE_TIMEOUT: Duration = Duration::from_millis(1_800);
pub(crate) const DICTATION_RESOLVE_WAIT: Duration = Duration::from_millis(150);
pub(crate) const DEFAULT_MAX_NODES: usize = 300;
pub(crate) const DEFAULT_MAX_CHARS: usize = 3_000;
pub(crate) const ABSOLUTE_MAX_CHARS: usize = 6_000;
pub(crate) const SUMMARY_PREVIEW_CHARS: usize = 500;

#[derive(Clone, Copy, Debug)]
pub(crate) struct ActivationTarget {
    pub(crate) window_handle: isize,
    pub(crate) process_id: u32,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct CaptureOptions {
    pub(crate) deadline: Instant,
    pub(crate) max_nodes: usize,
    pub(crate) max_chars: usize,
    pub(crate) debug: bool,
}

impl Default for CaptureOptions {
    fn default() -> Self {
        Self {
            deadline: Instant::now() + CAPTURE_TIMEOUT,
            max_nodes: DEFAULT_MAX_NODES,
            max_chars: DEFAULT_MAX_CHARS,
            debug: false,
        }
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
    AccessDenied,
    TimedOut,
    #[cfg_attr(windows, allow(dead_code))]
    Unsupported,
    Failed,
}

#[derive(Clone, Copy, Debug, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ContextSource {
    OcrWithUia,
    OcrOnly,
    #[default]
    UiaOnly,
}

#[derive(Clone, Copy, Debug, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub(crate) enum OcrCaptureMode {
    #[default]
    Adaptive,
    FullWindow,
    FallbackFullWindow,
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
    pub(crate) const FULL: Self = Self {
        left: 0.0,
        top: 0.0,
        right: 1.0,
        bottom: 1.0,
    };

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

    pub(crate) fn is_valid(self) -> bool {
        self.right - self.left > f32::EPSILON && self.bottom - self.top > f32::EPSILON
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
    pub(crate) app_name: String,
    pub(crate) process_name: String,
    pub(crate) process_id: u32,
    pub(crate) window_title: Option<String>,
    pub(crate) selected_text: Option<String>,
    pub(crate) focused_text: Option<String>,
    pub(crate) nearby_text: Vec<String>,
    pub(crate) document_text: Vec<String>,
    pub(crate) ocr_text: Vec<String>,
    pub(crate) full_window_ocr_text: Vec<String>,
    pub(crate) ocr_blocks: Vec<OcrTextBlock>,
    pub(crate) context_source: ContextSource,
    pub(crate) ocr_capture_mode: Option<OcrCaptureMode>,
    pub(crate) ocr_region: Option<NormalizedRegion>,
    pub(crate) screenshot_width: u32,
    pub(crate) screenshot_height: u32,
    pub(crate) screenshot_elapsed_ms: u64,
    pub(crate) model_init_ms: u64,
    pub(crate) ocr_elapsed_ms: u64,
    pub(crate) screenshot_data_url: Option<String>,
    pub(crate) diagnostics: Vec<String>,
    pub(crate) elapsed_ms: u64,
    pub(crate) truncated: bool,
    pub(crate) visited_nodes: usize,
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
        if let Some(text) = self
            .selected_text
            .as_deref()
            .filter(|value| !value.is_empty())
        {
            lines.push(format!("当前选中内容：{text}"));
        }
        if let Some(text) = self
            .focused_text
            .as_deref()
            .filter(|value| !value.is_empty())
        {
            lines.push(format!("当前输入区域：{text}"));
        }
        if !self.ocr_text.is_empty() {
            lines.push(format!("窗口可见文字：{}", self.ocr_text.join("\n")));
        }
        if !self.nearby_text.is_empty() {
            lines.push(format!("焦点附近内容：{}", self.nearby_text.join("\n")));
        }
        if !self.document_text.is_empty() && self.ocr_text.is_empty() {
            lines.push(format!("辅助功能正文：{}", self.document_text.join("\n")));
        }
        lines.join("\n")
    }

    pub(crate) fn has_content(&self) -> bool {
        !self.app_name.is_empty()
            || self
                .window_title
                .as_deref()
                .is_some_and(|value| !value.is_empty())
            || self
                .selected_text
                .as_deref()
                .is_some_and(|value| !value.is_empty())
            || self
                .focused_text
                .as_deref()
                .is_some_and(|value| !value.is_empty())
            || !self.nearby_text.is_empty()
            || !self.document_text.is_empty()
            || !self.ocr_text.is_empty()
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ActiveAppContextSummary {
    pub(crate) status: CaptureStatus,
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
            CaptureStatus::AccessDenied,
            CaptureStatus::TimedOut,
            CaptureStatus::Unsupported,
            CaptureStatus::Failed,
        ] {
            let context = CapturedActiveAppContext {
                status,
                focused_text: Some("不应发送".into()),
                ..Default::default()
            };
            assert!(context.format_for_prompt().is_empty());
        }
    }

    #[test]
    fn prompt_prefers_ocr_to_uia_document_and_keeps_fixed_order() {
        let context = CapturedActiveAppContext {
            status: CaptureStatus::Captured,
            app_name: "Code".into(),
            window_title: Some("ocr.rs".into()),
            selected_text: Some("选择".into()),
            focused_text: Some("焦点".into()),
            ocr_text: vec!["OCR 正文".into()],
            nearby_text: vec!["附近".into()],
            document_text: vec!["不应拼入的 UIA 正文".into()],
            ..Default::default()
        };
        let prompt = context.format_for_prompt();
        let expected = [
            "应用：",
            "窗口：",
            "当前选中内容：",
            "当前输入区域：",
            "窗口可见文字：",
            "焦点附近内容：",
        ];
        let mut previous = 0;
        for label in expected {
            let index = prompt.find(label).expect("label should exist");
            assert!(index >= previous);
            previous = index;
        }
        assert!(!prompt.contains("不应拼入"));
    }

    #[test]
    fn prompt_uses_uia_document_when_ocr_is_empty() {
        let context = CapturedActiveAppContext {
            status: CaptureStatus::Captured,
            document_text: vec!["UIA 兜底正文".into()],
            ..Default::default()
        };
        assert!(context
            .format_for_prompt()
            .contains("辅助功能正文：UIA 兜底正文"));
    }

    #[test]
    fn runtime_summary_never_serializes_image_or_ocr_boxes() {
        let context = CapturedActiveAppContext {
            status: CaptureStatus::Captured,
            screenshot_data_url: Some("data:image/png;base64,secret".into()),
            ocr_blocks: vec![OcrTextBlock {
                text: "secret".into(),
                confidence: 1.0,
                bounds: NormalizedRegion::FULL,
            }],
            ..Default::default()
        };
        let value = serde_json::to_value(ActiveAppContextSummary::from(&context)).unwrap();
        assert!(value.get("screenshotDataUrl").is_none());
        assert!(value.get("ocrBlocks").is_none());
    }
}
