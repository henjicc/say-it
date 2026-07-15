use std::collections::{HashSet, VecDeque};
use std::path::Path;
use std::time::{Duration, Instant};

use uiautomation::core::UICacheRequest;
use uiautomation::patterns::{UITextPattern, UIValuePattern};
use uiautomation::types::{ControlType, Handle, TreeScope, UIProperty};
use uiautomation::{UIAutomation, UIElement, UITreeWalker};
use windows::core::PWSTR;
use windows::Win32::Foundation::{CloseHandle, HWND, RECT};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowRect, GetWindowTextLengthW, GetWindowTextW,
    GetWindowThreadProcessId,
};

use super::model::{
    ActivationTarget, CaptureOptions, CaptureStatus, CapturedActiveAppContext, ContextSource,
    NormalizedRegion,
};
use super::normalize::{enforce_total_budget, normalize_text, push_unique, truncate_chars};
use super::ActiveAppContextProvider;
use super::{ocr, screen_capture};

const FIELD_CHAR_LIMIT: usize = 1_000;
const DOCUMENT_BLOCK_LIMIT: usize = 2_000;
const MAX_ANCESTORS: usize = 16;
const MAX_NEARBY_ANCESTORS: usize = 4;
const MAX_VISIBLE_RANGES: usize = 4;
const DEADLINE_RESERVE: Duration = Duration::from_millis(120);

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
    Some(ActivationTarget {
        window_handle: window.0 as isize,
        process_id,
    })
}

impl ActiveAppContextProvider for WindowsActiveAppContextProvider {
    fn capture(
        &self,
        target: ActivationTarget,
        blocked_apps: &[String],
        options: CaptureOptions,
    ) -> CapturedActiveAppContext {
        let started = Instant::now();
        let process_name = process_name(target.process_id).unwrap_or_default();
        let app_name = Path::new(&process_name)
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or(&process_name)
            .to_string();
        let mut context = CapturedActiveAppContext {
            app_name,
            process_name,
            process_id: target.process_id,
            window_title: window_title(target.window_handle),
            ..Default::default()
        };

        if is_blocked(&context, blocked_apps) {
            context.status = CaptureStatus::Blocked;
            context.elapsed_ms = started.elapsed().as_millis() as u64;
            return context;
        }
        if expired(options.deadline) {
            context.status = CaptureStatus::TimedOut;
            context.elapsed_ms = started.elapsed().as_millis() as u64;
            return context;
        }

        let automation = match UIAutomation::new() {
            Ok(value) => value,
            Err(error) => {
                return finish_ocr_only(
                    context,
                    target,
                    &options,
                    started,
                    error_status(&error.to_string()),
                );
            }
        };
        let cache = match create_property_cache(&automation) {
            Ok(value) => value,
            Err(error) => {
                return finish_ocr_only(
                    context,
                    target,
                    &options,
                    started,
                    error_status(&error.to_string()),
                );
            }
        };
        let root = match automation
            .element_from_handle_build_cache(Handle::from(target.window_handle), &cache)
        {
            Ok(value) => value,
            Err(error) => {
                return finish_ocr_only(
                    context,
                    target,
                    &options,
                    started,
                    error_status(&error.to_string()),
                );
            }
        };
        if cached_process_id(&root) != Some(target.process_id) {
            context.status = CaptureStatus::Failed;
            context.elapsed_ms = started.elapsed().as_millis() as u64;
            return context;
        }
        let cached_window_title = root.get_cached_name().ok().and_then(|value| {
            let (value, truncated) = truncate_chars(&normalize_text(&value), FIELD_CHAR_LIMIT);
            context.truncated |= truncated;
            (!value.is_empty()).then_some(value)
        });
        if cached_window_title.is_some() {
            context.window_title = cached_window_title;
        }
        if context.app_name.is_empty() {
            context.app_name = context
                .window_title
                .clone()
                .unwrap_or_else(|| format!("PID {}", target.process_id));
        }

        let walker = match automation.get_content_view_walker() {
            Ok(value) => value,
            Err(error) => {
                return finish_ocr_only(
                    context,
                    target,
                    &options,
                    started,
                    error_status(&error.to_string()),
                );
            }
        };
        let focus = automation
            .get_focused_element_build_cache(&cache)
            .ok()
            .filter(|element| {
                element_matches_target(element, target.process_id, &context.process_name)
            });
        let focus_password_state = focus
            .as_ref()
            .and_then(|element| element.is_cached_password().ok());
        if is_confirmed_sensitive(focus_password_state) {
            context.status = CaptureStatus::Sensitive;
            context.elapsed_ms = started.elapsed().as_millis() as u64;
            return context;
        }
        let focus_region = focus
            .as_ref()
            .and_then(|element| normalized_focus_region(element, target.window_handle));

        let focus_process_id = focus.as_ref().and_then(cached_process_id);
        let mut collector =
            Collector::new(context, with_deadline_reserve(options), focus_process_id);
        let focused_document = focus
            .as_ref()
            .filter(|_| focus_password_state != Some(true))
            .and_then(|focus| collector.collect_focus(focus, &walker, &cache));
        let ocr_succeeded = if expired(options.deadline) {
            false
        } else {
            apply_ocr(
                &mut collector.context,
                target.window_handle,
                focus_region,
                options.debug,
                options.deadline,
            )
        };
        if !ocr_succeeded && !collector.is_expired() {
            if let Some(document) = focused_document.as_ref() {
                collector.collect_document_element(document);
            }
        }
        if !ocr_succeeded && !collector.is_expired() {
            collector.collect_visible_tree(&root, &walker, &cache);
        }
        let mut context = collector.finish();
        enforce_total_budget(&mut context, options.max_chars);
        context.elapsed_ms = started.elapsed().as_millis() as u64;
        context.status = completed_status(&context, options.deadline);
        context
    }
}

fn finish_ocr_only(
    mut context: CapturedActiveAppContext,
    target: ActivationTarget,
    options: &CaptureOptions,
    started: Instant,
    fallback_status: CaptureStatus,
) -> CapturedActiveAppContext {
    let succeeded = !expired(options.deadline)
        && apply_ocr(
            &mut context,
            target.window_handle,
            None,
            options.debug,
            options.deadline,
        );
    enforce_total_budget(&mut context, options.max_chars);
    context.elapsed_ms = started.elapsed().as_millis() as u64;
    context.status = if succeeded {
        CaptureStatus::Captured
    } else if expired(options.deadline) {
        CaptureStatus::TimedOut
    } else {
        fallback_status
    };
    context
}

fn apply_ocr(
    context: &mut CapturedActiveAppContext,
    window_handle: isize,
    focus_region: Option<NormalizedRegion>,
    debug: bool,
    deadline: Instant,
) -> bool {
    let captured = match screen_capture::capture_window(window_handle) {
        Ok(value) => value,
        Err(error) => {
            context.diagnostics.push(error);
            return false;
        }
    };
    context.screenshot_width = captured.image.width();
    context.screenshot_height = captured.image.height();
    context.screenshot_elapsed_ms = captured.elapsed_ms;
    context.diagnostics.extend(captured.compatibility_notes);
    if debug {
        context.screenshot_data_url = ocr::png_data_url(&captured.image).ok();
    }
    let remaining = deadline.saturating_duration_since(Instant::now());
    let output = match ocr::run_pipeline(captured.image, focus_region, debug, remaining) {
        Ok(value) => value,
        Err(error) => {
            context.diagnostics.push(error);
            return false;
        }
    };
    context.ocr_text = output.text;
    context.full_window_ocr_text = output.full_window_text;
    context.ocr_blocks = output.blocks;
    context.ocr_capture_mode = Some(output.mode);
    context.ocr_region = Some(output.region);
    context.model_init_ms = output.model_init_ms;
    context.ocr_elapsed_ms = output.elapsed_ms;
    context.truncated |= output.truncated;
    context.context_source = if context.selected_text.is_some()
        || context.focused_text.is_some()
        || !context.nearby_text.is_empty()
    {
        ContextSource::OcrWithUia
    } else {
        ContextSource::OcrOnly
    };
    !context.ocr_text.is_empty()
}

fn normalized_focus_region(element: &UIElement, window_handle: isize) -> Option<NormalizedRegion> {
    let focus = element.get_cached_bounding_rectangle().ok()?;
    let mut window = RECT::default();
    unsafe {
        GetWindowRect(HWND(window_handle as *mut std::ffi::c_void), &mut window).ok()?;
    }
    let width = (window.right - window.left) as f32;
    let height = (window.bottom - window.top) as f32;
    if width <= 1.0 || height <= 1.0 {
        return None;
    }
    let region = NormalizedRegion {
        left: (focus.get_left() - window.left) as f32 / width,
        top: (focus.get_top() - window.top) as f32 / height,
        right: (focus.get_right() - window.left) as f32 / width,
        bottom: (focus.get_bottom() - window.top) as f32 / height,
    }
    .clamped();
    region.is_valid().then_some(region)
}

struct Collector {
    context: CapturedActiveAppContext,
    options: CaptureOptions,
    seen: HashSet<String>,
    allowed_process_ids: HashSet<u32>,
}

impl Collector {
    fn new(
        context: CapturedActiveAppContext,
        options: CaptureOptions,
        focus_process_id: Option<u32>,
    ) -> Self {
        let mut seen = HashSet::new();
        for value in [
            context.app_name.as_str(),
            context.window_title.as_deref().unwrap_or(""),
        ] {
            if !value.is_empty() {
                seen.insert(value.to_lowercase());
            }
        }
        let mut allowed_process_ids = HashSet::from([context.process_id]);
        if let Some(process_id) = focus_process_id {
            allowed_process_ids.insert(process_id);
        }
        Self {
            context,
            options,
            seen,
            allowed_process_ids,
        }
    }

    fn is_expired(&self) -> bool {
        expired(self.options.deadline) || self.context.visited_nodes >= self.options.max_nodes
    }

    fn visit(&mut self) -> bool {
        if self.is_expired() {
            return false;
        }
        self.context.visited_nodes += 1;
        true
    }

    fn collect_focus(
        &mut self,
        focus: &UIElement,
        walker: &UITreeWalker,
        cache: &UICacheRequest,
    ) -> Option<UIElement> {
        if !self.visit() || is_cached_sensitive(focus) {
            return None;
        }
        if let Ok(pattern) = focus.get_pattern::<UITextPattern>() {
            for range in pattern.get_selection().unwrap_or_default() {
                if self.is_expired() {
                    break;
                }
                if let Ok(text) = range.get_text(FIELD_CHAR_LIMIT as i32) {
                    let (text, truncated) =
                        truncate_chars(&normalize_text(&text), FIELD_CHAR_LIMIT);
                    self.context.truncated |= truncated;
                    if !text.is_empty() {
                        self.seen.insert(text.to_lowercase());
                        self.context.selected_text = Some(text);
                        break;
                    }
                }
            }
            if self.context.focused_text.is_none() {
                if let Ok(range) = pattern.get_document_range() {
                    if let Ok(text) = range.get_text(FIELD_CHAR_LIMIT as i32) {
                        let (text, truncated) =
                            truncate_chars(&normalize_text(&text), FIELD_CHAR_LIMIT);
                        self.context.truncated |= truncated;
                        if !text.is_empty() && self.seen.insert(text.to_lowercase()) {
                            self.context.focused_text = Some(text);
                        }
                    }
                }
            }
        }
        if self.context.focused_text.is_none() && !self.is_expired() {
            if let Ok(pattern) = focus.get_pattern::<UIValuePattern>() {
                if let Ok(value) = pattern.get_value() {
                    let (value, truncated) =
                        truncate_chars(&normalize_text(&value), FIELD_CHAR_LIMIT);
                    self.context.truncated |= truncated;
                    if !value.is_empty() && self.seen.insert(value.to_lowercase()) {
                        self.context.focused_text = Some(value);
                    }
                }
            }
        }
        self.collect_cached_element_metadata(focus);

        let mut current = focus.clone();
        for depth in 0..MAX_ANCESTORS {
            if self.is_expired() {
                break;
            }
            let Ok(parent) = walker.get_parent_build_cache(&current, cache) else {
                break;
            };
            if !self.allows_process(&parent) {
                break;
            }
            let sensitive = is_cached_sensitive(&parent);
            if parent.get_cached_control_type().ok() == Some(ControlType::Document) {
                return (!sensitive).then_some(parent);
            }
            if !sensitive {
                if depth < MAX_NEARBY_ANCESTORS {
                    self.collect_cached_element_metadata(&parent);
                    if !self.is_expired() {
                        if let Ok(previous) =
                            walker.get_previous_sibling_build_cache(&current, cache)
                        {
                            self.collect_cached_element_metadata(&previous);
                        }
                    }
                    if !self.is_expired() {
                        if let Ok(next) = walker.get_next_sibling_build_cache(&current, cache) {
                            self.collect_cached_element_metadata(&next);
                        }
                    }
                }
            }
            current = parent;
        }
        None
    }

    fn collect_cached_element_metadata(&mut self, element: &UIElement) {
        if !self.visit()
            || is_cached_sensitive(element)
            || element.is_cached_offscreen().unwrap_or(true)
        {
            return;
        }
        self.push_nearby(element.get_cached_name().ok());
        if self.is_expired() {
            return;
        }
        self.push_nearby(element.get_cached_help_text().ok());
        if self.is_expired() {
            return;
        }
        self.push_nearby(element.get_cached_item_type().ok());
        if self.is_expired() {
            return;
        }
        self.push_nearby(element.get_cached_localized_control_type().ok());
    }

    fn push_nearby(&mut self, value: Option<String>) {
        if let Some(value) = value {
            self.context.truncated |= push_unique(
                &mut self.context.nearby_text,
                &mut self.seen,
                &value,
                FIELD_CHAR_LIMIT,
            );
        }
    }

    fn collect_document_element(&mut self, candidate: &UIElement) {
        if self.is_expired() || is_cached_sensitive(candidate) {
            return;
        }
        let Ok(pattern) = candidate.get_pattern::<UITextPattern>() else {
            return;
        };
        if self.is_expired() {
            return;
        }
        let ranges = pattern.get_visible_ranges().unwrap_or_default();
        if ranges.is_empty() {
            if !self.is_expired() {
                if let Ok(range) = pattern.get_document_range() {
                    self.push_document_range(&range);
                }
            }
            return;
        }
        for range in ranges.into_iter().take(MAX_VISIBLE_RANGES) {
            self.push_document_range(&range);
            if self.is_expired() || self.has_enough_text() {
                break;
            }
        }
        self.context.truncated |= self.has_enough_text();
    }

    fn push_document_range(&mut self, range: &uiautomation::patterns::UITextRange) {
        let Ok(text) = range.get_text(DOCUMENT_BLOCK_LIMIT as i32) else {
            return;
        };
        self.context.truncated |= push_unique(
            &mut self.context.document_text,
            &mut self.seen,
            &text,
            DOCUMENT_BLOCK_LIMIT,
        );
    }

    fn collect_visible_tree(
        &mut self,
        root: &UIElement,
        walker: &UITreeWalker,
        cache: &UICacheRequest,
    ) {
        let mut queue = VecDeque::from([root.clone()]);
        let mut document_collected = !self.context.document_text.is_empty();
        while let Some(element) = queue.pop_front() {
            if !self.visit() || self.has_enough_text() {
                break;
            }
            let sensitive = is_cached_sensitive(&element);
            let visible = !element.is_cached_offscreen().unwrap_or(true);
            if !sensitive && visible {
                if !document_collected
                    && element.get_cached_control_type().ok() == Some(ControlType::Document)
                {
                    self.collect_document_element(&element);
                    document_collected = !self.context.document_text.is_empty();
                }
                if let Ok(name) = element.get_cached_name() {
                    self.context.truncated |= push_unique(
                        &mut self.context.document_text,
                        &mut self.seen,
                        &name,
                        FIELD_CHAR_LIMIT,
                    );
                }
            }
            if sensitive {
                continue;
            }
            self.enqueue_children(&element, walker, cache, &mut queue);
        }
    }

    fn finish(mut self) -> CapturedActiveAppContext {
        self.context.truncated |= self.context.visited_nodes >= self.options.max_nodes
            || expired(self.options.deadline)
            || self.has_enough_text();
        self.context
    }

    fn has_enough_text(&self) -> bool {
        let focused = self
            .context
            .selected_text
            .as_deref()
            .into_iter()
            .chain(self.context.focused_text.as_deref())
            .map(|value| value.chars().count())
            .sum::<usize>();
        let nearby = self
            .context
            .nearby_text
            .iter()
            .chain(&self.context.document_text)
            .map(|value| value.chars().count())
            .sum::<usize>();
        focused + nearby >= self.options.max_chars
    }

    fn enqueue_children(
        &self,
        element: &UIElement,
        walker: &UITreeWalker,
        cache: &UICacheRequest,
        queue: &mut VecDeque<UIElement>,
    ) {
        if self.is_expired() {
            return;
        }
        let Ok(mut current) = walker.get_first_child_build_cache(element, cache) else {
            return;
        };
        loop {
            if self.is_expired()
                || queue.len() + self.context.visited_nodes >= self.options.max_nodes
            {
                break;
            }
            queue.push_back(current.clone());
            let Ok(next) = walker.get_next_sibling_build_cache(&current, cache) else {
                break;
            };
            current = next;
        }
    }

    fn allows_process(&mut self, element: &UIElement) -> bool {
        let Some(process_id) = cached_process_id(element) else {
            return false;
        };
        if self.allowed_process_ids.contains(&process_id) {
            return true;
        }
        let same_executable = process_name(process_id)
            .is_some_and(|name| same_executable_name(&name, &self.context.process_name));
        if same_executable {
            self.allowed_process_ids.insert(process_id);
        }
        same_executable
    }
}

fn is_cached_sensitive(element: &UIElement) -> bool {
    element.is_cached_password().unwrap_or(true)
}

fn is_confirmed_sensitive(password_state: Option<bool>) -> bool {
    password_state == Some(true)
}

fn cached_process_id(element: &UIElement) -> Option<u32> {
    u32::try_from(element.get_cached_process_id().ok()?).ok()
}

fn element_matches_target(
    element: &UIElement,
    target_process_id: u32,
    target_process_name: &str,
) -> bool {
    let Some(process_id) = cached_process_id(element) else {
        return false;
    };
    process_id == target_process_id
        || process_name(process_id)
            .is_some_and(|name| same_executable_name(&name, target_process_name))
}

fn same_executable_name(candidate: &str, target: &str) -> bool {
    !target.is_empty() && candidate.eq_ignore_ascii_case(target)
}

fn create_property_cache(automation: &UIAutomation) -> uiautomation::Result<UICacheRequest> {
    let cache = automation.create_cache_request()?;
    for property in [
        UIProperty::ProcessId,
        UIProperty::BoundingRectangle,
        UIProperty::ControlType,
        UIProperty::LocalizedControlType,
        UIProperty::Name,
        UIProperty::HelpText,
        UIProperty::IsPassword,
        UIProperty::ItemType,
        UIProperty::IsOffscreen,
    ] {
        cache.add_property(property)?;
    }
    cache.set_tree_scope(TreeScope::Element)?;
    Ok(cache)
}

fn with_deadline_reserve(mut options: CaptureOptions) -> CaptureOptions {
    options.deadline = options
        .deadline
        .checked_sub(DEADLINE_RESERVE)
        .unwrap_or(options.deadline);
    options
}

fn completed_status(context: &CapturedActiveAppContext, hard_deadline: Instant) -> CaptureStatus {
    if context.has_content() {
        CaptureStatus::Captured
    } else if expired(hard_deadline) {
        CaptureStatus::TimedOut
    } else {
        CaptureStatus::Empty
    }
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

fn error_status(error: &str) -> CaptureStatus {
    let error = error.to_lowercase();
    if error.contains("access") || error.contains("0x80070005") {
        CaptureStatus::AccessDenied
    } else {
        CaptureStatus::Failed
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
    fn deadline_reserve_leaves_time_to_return_partial_context() {
        let hard_deadline = Instant::now() + Duration::from_millis(800);
        let options = with_deadline_reserve(CaptureOptions {
            deadline: hard_deadline,
            max_nodes: 300,
            max_chars: 3_000,
            debug: false,
        });
        assert_eq!(
            hard_deadline.duration_since(options.deadline),
            DEADLINE_RESERVE
        );
    }

    #[test]
    fn useful_partial_context_is_not_discarded_at_soft_deadline() {
        let context = CapturedActiveAppContext {
            app_name: "Code".into(),
            window_title: Some("windows.rs".into()),
            truncated: true,
            ..Default::default()
        };
        assert_eq!(
            completed_status(&context, Instant::now() - Duration::from_millis(1)),
            CaptureStatus::Captured
        );
    }

    #[test]
    fn chromium_child_processes_match_by_executable_name() {
        assert!(same_executable_name("chrome.exe", "Chrome.exe"));
        assert!(same_executable_name("Code.exe", "code.exe"));
        assert!(!same_executable_name("msedge.exe", "chrome.exe"));
        assert!(!same_executable_name("chrome.exe", ""));
    }

    #[test]
    fn only_confirmed_password_focus_stops_screenshot() {
        assert!(is_confirmed_sensitive(Some(true)));
        assert!(!is_confirmed_sensitive(Some(false)));
        assert!(!is_confirmed_sensitive(None));
    }
}
