use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

pub(crate) const NATIVE_TEXT_CAPTURE_TIMEOUT: Duration = Duration::from_millis(800);
pub(crate) const OCR_CAPTURE_TIMEOUT: Duration = Duration::from_secs(5);
pub(crate) const DICTATION_RESOLVE_WAIT: Duration = Duration::from_millis(150);
pub(crate) const DEFAULT_MAX_CHARS: usize = 3_000;
/// 软件提供的文字不足这么多字时截图识别补上，见 `CapturedActiveAppContext::needs_ocr_supplement`。
const NATIVE_ENOUGH_CHARS: usize = 200;
pub(crate) const ABSOLUTE_MAX_CHARS: usize = 6_000;
pub(crate) const SUMMARY_PREVIEW_CHARS: usize = 500;
/// 窗口截图长边像素上限的默认值；调试窗口可临时覆盖，见 `CaptureOptions::max_capture_side_override`。
pub(crate) const DEFAULT_MAX_CAPTURE_SIDE: u32 = 1_600;

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ActiveAppContextExtractionMethod {
    NativeText,
    Ocr,
}

impl Default for ActiveAppContextExtractionMethod {
    fn default() -> Self {
        if cfg!(target_os = "macos") {
            Self::Ocr
        } else {
            Self::NativeText
        }
    }
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


/// 前台软件的轻量标识。只读取本进程可直接取得的窗口元信息，不涉及正文提取、
/// UIA 或 OCR，因此可以在每次听写启动时无条件调用。
#[derive(Clone, Debug, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AppIdentity {
    /// 可执行文件名，如 `Code.exe`。应用规则以它为匹配键。
    pub(crate) process_name: String,
    /// 去掉扩展名的显示名，如 `Code`。
    pub(crate) app_name: String,
    pub(crate) window_title: Option<String>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ActivationTarget {
    pub(crate) window_handle: isize,
    pub(crate) process_id: u32,
    /// 全局快捷键触发时鼠标仍通常停在刚点击的输入区。该坐标仅在本次捕获中用于
    /// 定点定位辅助功能元素，不会持久化或发送到模型。
    #[cfg_attr(not(windows), allow(dead_code))]
    pub(crate) cursor_position: Option<(i32, i32)>,
}

#[derive(Clone)]
pub(crate) struct CaptureOptions {
    pub(crate) deadline: Instant,
    pub(crate) max_chars: usize,
    pub(crate) debug: bool,
    #[cfg_attr(not(windows), allow(dead_code))]
    pub(crate) occluding_window_handle: Option<isize>,
    pub(crate) method: ActiveAppContextExtractionMethod,
    pub(crate) ocr_provider: crate::providers::capabilities::OcrProvider,
    pub(crate) selection_only: bool,
    /// 读不到选区时能否按复制键从剪贴板读。说话过程中不能按：用户可能还按着快捷键。
    #[cfg_attr(not(windows), allow(dead_code))]
    pub(crate) copy_selection: bool,
    /// 仅调试窗口会设置：覆盖截图长边像素上限（默认见 `screen_capture::MAX_CAPTURE_SIDE`）。
    pub(crate) max_capture_side_override: Option<u32>,
}

impl CaptureOptions {
    pub(crate) fn for_method(
        method: ActiveAppContextExtractionMethod,
        ocr_provider: crate::providers::capabilities::OcrProvider,
    ) -> Self {
        Self {
            deadline: Instant::now() + method.timeout(),
            max_chars: DEFAULT_MAX_CHARS,
            debug: false,
            occluding_window_handle: None,
            method,
            ocr_provider,
            selection_only: false,
            copy_selection: true,
            max_capture_side_override: None,
        }
    }

    /// 听写读软件上下文的固定做法：Windows 先读软件提供的文字（不够时另外截图识别补，见
    /// `CapturedActiveAppContext::needs_ocr_supplement`）；macOS 截图识别。
    pub(crate) fn for_context(ocr_provider: crate::providers::capabilities::OcrProvider) -> Self {
        Self::for_method(ActiveAppContextExtractionMethod::default(), ocr_provider)
    }

    /// 同样的设置改成截图识别，截止时间从现在重新算。
    #[cfg_attr(not(windows), allow(dead_code))]
    pub(crate) fn as_ocr(&self, ocr_provider: crate::providers::capabilities::OcrProvider) -> Self {
        Self {
            deadline: Instant::now() + OCR_CAPTURE_TIMEOUT,
            method: ActiveAppContextExtractionMethod::Ocr,
            ocr_provider,
            ..self.clone()
        }
    }
}

impl Default for CaptureOptions {
    fn default() -> Self {
        Self::for_method(
            ActiveAppContextExtractionMethod::default(),
            crate::providers::capabilities::OcrProvider::default_local(),
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
    #[cfg_attr(any(windows, target_os = "macos"), allow(dead_code))]
    Unsupported,
    Failed,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[cfg_attr(not(windows), allow(dead_code))]
#[serde(rename_all = "camelCase")]
pub(crate) enum ContextSource {
    Accessibility,
    Ia2Text,
    UiaTextPattern,
    Win32Message,
    OfficeNative,
    Msaa,
    ClipboardDeep,
    Ocr,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ContextBounds {
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) width: f64,
    pub(crate) height: f64,
}

// OCR 数据结构上提到 `crate::ocr`（providers 与本模块共用）；这里保留原路径的再导出。
#[cfg(any(windows, test))]
pub(crate) use crate::ocr::NormalizedRegion;
pub(crate) use crate::ocr::OcrTextBlock;

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
    pub(crate) selection_bounds: Option<ContextBounds>,
    pub(crate) selection_editable: Option<bool>,
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
    /// 开了动态上下文时，说话过程中补充的内容。
    pub(crate) dynamic: Vec<super::dynamic::DynamicEntry>,
    /// 开了动态上下文时，一串（开口后的毫秒数，当时已识别的字数），见 `annotate_references`。
    #[serde(skip)]
    pub(crate) spoken: Vec<(u64, usize)>,
}

impl CapturedActiveAppContext {
    #[cfg(any(windows, target_os = "macos"))]
    pub(crate) fn with_status(status: CaptureStatus) -> Self {
        Self {
            status,
            ..Self::default()
        }
    }

    /// 给整理结果里的“这个”“这里”补上说话时选中或鼠标指着的内容，见 `references.rs`。
    pub(crate) fn annotate_references(&self, raw: &str, output: &str) -> String {
        super::references::annotate_references(raw, output, &self.dynamic, &self.spoken)
    }

    /// 软件提供的文字（选区、输入框、可见文字、文档文字）不足这么多字时要截图识别补上：自绘界面、
    /// 画布、远程桌面往往只给得出窗口标题或一个输入框。密码框、黑名单不补。
    #[cfg_attr(not(windows), allow(dead_code))]
    pub(crate) fn needs_ocr_supplement(&self) -> bool {
        if matches!(self.status, CaptureStatus::Sensitive | CaptureStatus::Blocked) {
            return false;
        }
        let chars: usize = [&self.selected_text, &self.focused_text, &self.caret_context]
            .into_iter()
            .flatten()
            .chain(self.visible_text.iter())
            .chain(self.document_text.iter())
            .map(|text| text.chars().count())
            .sum();
        chars < NATIVE_ENOUGH_CHARS
    }

    /// 把截图识别到的窗口文字补进来，已经读到的选区、输入框照留。识别失败或没识别到字时原样返回；
    /// 原来一个字都没读到时，来源记为截图识别。
    #[cfg_attr(not(windows), allow(dead_code))]
    pub(crate) fn with_ocr(mut self, ocr: &CapturedActiveAppContext) -> Self {
        if ocr.status != CaptureStatus::Captured || ocr.ocr_text.is_empty() {
            return self;
        }
        let had_text = self.status == CaptureStatus::Captured && !self.text_lines().is_empty()
            || self.selected_text.is_some()
            || self.focused_text.is_some();
        self.ocr_text = ocr.ocr_text.clone();
        self.ocr_blocks = ocr.ocr_blocks.clone();
        self.screenshot_width = ocr.screenshot_width;
        self.screenshot_height = ocr.screenshot_height;
        self.screenshot_elapsed_ms = ocr.screenshot_elapsed_ms;
        self.screenshot_data_url = ocr.screenshot_data_url.clone();
        self.model_init_ms = ocr.model_init_ms;
        self.ocr_elapsed_ms = ocr.ocr_elapsed_ms;
        self.truncated |= ocr.truncated;
        self.status = CaptureStatus::Captured;
        if !had_text {
            self.source = Some(ContextSource::Ocr);
        }
        self
    }

    /// 窗口里读到的大段文字：原生读取的可见文字、文档文字和 OCR 文字。
    pub(crate) fn text_lines(&self) -> Vec<String> {
        [&self.visible_text, &self.document_text, &self.ocr_text]
            .into_iter()
            .flatten()
            .cloned()
            .collect()
    }

    /// 说话过程中的操作排在应用、窗口、选区之后，大段窗口文字之前：本地整理模型只看开头一段。
    /// 有这一段时整体仍守住默认字数上限，超出的从窗口文字末尾截掉。
    pub(crate) fn format_for_prompt(&self) -> String {
        self.format_with_dynamic(&self.dynamic)
    }

    /// 带上另给的动态记录排版：听写过程中记录还在增加，不必为此复制整份上下文。
    pub(crate) fn format_with_dynamic(&self, entries: &[super::dynamic::DynamicEntry]) -> String {
        let captured = self.status == CaptureStatus::Captured;
        let seen = match captured {
            true => self.selected_text.iter().cloned().chain(self.text_lines()).collect(),
            false => Vec::new(),
        };
        let dynamic = super::dynamic::format_entries(entries, &seen);
        if !captured {
            return dynamic;
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
        if !dynamic.is_empty() {
            lines.push(dynamic.clone());
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
        let text = lines.join("\n");
        match dynamic.is_empty() {
            true => text,
            false => super::normalize::truncate_chars(&text, DEFAULT_MAX_CHARS).0,
        }
    }

    #[cfg(windows)]
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
        if matches!(
            self.status,
            CaptureStatus::Blocked | CaptureStatus::Sensitive
        ) || !self.has_prompt_metadata()
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
    fn missing_method_defaults_to_platform_method() {
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
        assert_eq!(
            value.method,
            if cfg!(target_os = "macos") {
                ActiveAppContextExtractionMethod::Ocr
            } else {
                ActiveAppContextExtractionMethod::NativeText
            }
        );
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
    fn scarce_software_text_is_supplemented_by_ocr_and_kept() {
        let native = CapturedActiveAppContext {
            status: CaptureStatus::Captured,
            source: Some(ContextSource::UiaTextPattern),
            app_name: "画图".into(),
            focused_text: Some("标题".into()),
            ..Default::default()
        };
        assert!(native.needs_ocr_supplement());
        // 选区也算软件提供的文字，够了就不截图。
        let selected = CapturedActiveAppContext { selected_text: Some("字".repeat(NATIVE_ENOUGH_CHARS)), ..native.clone() };
        assert!(!selected.needs_ocr_supplement());
        // 密码框不补。
        let sensitive = CapturedActiveAppContext { status: CaptureStatus::Sensitive, ..native.clone() };
        assert!(!sensitive.needs_ocr_supplement());

        let ocr = CapturedActiveAppContext {
            status: CaptureStatus::Captured,
            ocr_text: vec!["画布上的字".into()],
            ..Default::default()
        };
        let merged = native.clone().with_ocr(&ocr);
        assert_eq!(merged.focused_text.as_deref(), Some("标题"));
        assert_eq!(merged.ocr_text, vec!["画布上的字".to_string()]);
        assert_eq!(merged.source, Some(ContextSource::UiaTextPattern));
        // 软件一个字都没给（只拿到窗口信息、读取超时）时，来源记为截图识别。
        let timed_out = CapturedActiveAppContext { status: CaptureStatus::TimedOut, app_name: "画图".into(), ..Default::default() };
        let merged = timed_out.with_ocr(&ocr);
        assert_eq!((merged.status, merged.source), (CaptureStatus::Captured, Some(ContextSource::Ocr)));
        // 识别失败时原样保留已读到的。
        let failed = CapturedActiveAppContext { status: CaptureStatus::Failed, ..Default::default() };
        assert_eq!(native.clone().with_ocr(&failed).ocr_text, Vec::<String>::new());
    }

    /// 本地整理模型只看上下文开头一段，说话时的操作要排在大段窗口文字前面，总长仍守住上限。
    #[test]
    fn dynamic_entries_come_before_window_text_and_keep_the_budget() {
        use super::super::dynamic::{DynamicEntry, DynamicKind};
        let context = CapturedActiveAppContext {
            status: CaptureStatus::Captured,
            app_name: "Code".into(),
            ocr_text: vec!["字".repeat(DEFAULT_MAX_CHARS)],
            dynamic: vec![DynamicEntry { at_ms: 1500, kind: DynamicKind::Pointer { text: "导出".into(), exact: true } }],
            ..Default::default()
        };
        let text = context.format_for_prompt();
        assert!(text.starts_with("应用：Code\n说话时的操作"));
        assert!(text.contains("[1.5秒] 鼠标指着：导出\n窗口可见文字：字"));
        assert_eq!(text.chars().count(), DEFAULT_MAX_CHARS);
        // 开头那次没读到时，说话中读到的仍然有用。
        let failed = CapturedActiveAppContext { status: CaptureStatus::TimedOut, ..context };
        assert!(failed.format_for_prompt().ends_with("[1.5秒] 鼠标指着：导出"));
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
