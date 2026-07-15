use serde::Serialize;
use std::time::{Duration, Instant};

pub(crate) const CAPTURE_TIMEOUT: Duration = Duration::from_millis(800);
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
}

impl Default for CaptureOptions {
    fn default() -> Self {
        Self {
            deadline: Instant::now() + CAPTURE_TIMEOUT,
            max_nodes: DEFAULT_MAX_NODES,
            max_chars: DEFAULT_MAX_CHARS,
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
    AccessDenied,
    TimedOut,
    #[cfg_attr(windows, allow(dead_code))]
    Unsupported,
    Failed,
}

#[derive(Clone, Debug, Default)]
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
        if !self.nearby_text.is_empty() {
            lines.push(format!("焦点附近内容：{}", self.nearby_text.join("\n")));
        }
        if !self.document_text.is_empty() {
            lines.push(format!("当前可见正文：{}", self.document_text.join("\n")));
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
}
