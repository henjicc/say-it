use serde::Serialize;
use std::time::{Duration, Instant};

pub(crate) const CAPTURE_TIMEOUT: Duration = Duration::from_secs(5);
pub(crate) const DICTATION_RESOLVE_WAIT: Duration = Duration::from_millis(150);
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
    pub(crate) max_chars: usize,
    pub(crate) debug: bool,
    pub(crate) occluding_window_handle: Option<isize>,
}

impl Default for CaptureOptions {
    fn default() -> Self {
        Self {
            deadline: Instant::now() + CAPTURE_TIMEOUT,
            max_chars: DEFAULT_MAX_CHARS,
            debug: false,
            occluding_window_handle: None,
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
    TimedOut,
    #[cfg_attr(windows, allow(dead_code))]
    Unsupported,
    Failed,
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
    pub(crate) app_name: String,
    pub(crate) process_name: String,
    pub(crate) process_id: u32,
    pub(crate) window_title: Option<String>,
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
        if !self.ocr_text.is_empty() {
            lines.push(format!("窗口可见文字：{}", self.ocr_text.join("\n")));
        }
        lines.join("\n")
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
            ocr_text: vec!["内容".repeat(400)],
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
            CaptureStatus::TimedOut,
            CaptureStatus::Unsupported,
            CaptureStatus::Failed,
        ] {
            let context = CapturedActiveAppContext {
                status,
                ocr_text: vec!["不应发送".into()],
                ..Default::default()
            };
            assert!(context.format_for_prompt().is_empty());
        }
    }

    #[test]
    fn prompt_contains_only_window_metadata_and_ocr_text() {
        let context = CapturedActiveAppContext {
            status: CaptureStatus::Captured,
            app_name: "Code".into(),
            window_title: Some("ocr.rs".into()),
            ocr_text: vec!["OCR 正文".into()],
            ..Default::default()
        };
        assert_eq!(
            context.format_for_prompt(),
            "应用：Code\n窗口：ocr.rs\n窗口可见文字：OCR 正文"
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
