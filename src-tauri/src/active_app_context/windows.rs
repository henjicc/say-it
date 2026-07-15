use std::collections::{HashSet, VecDeque};
use std::path::Path;
use std::time::Instant;

use uiautomation::patterns::{UITextPattern, UIValuePattern};
use uiautomation::types::{ControlType, Handle};
use uiautomation::{UIAutomation, UIElement, UITreeWalker};
use windows::core::PWSTR;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

use super::model::{ActivationTarget, CaptureOptions, CaptureStatus, CapturedActiveAppContext};
use super::normalize::{enforce_total_budget, normalize_text, push_unique, truncate_chars};
use super::ActiveAppContextProvider;

const FIELD_CHAR_LIMIT: usize = 1_000;
const DOCUMENT_BLOCK_LIMIT: usize = 2_000;
const MAX_ANCESTORS: usize = 8;

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
                context.status = error_status(&error.to_string());
                context.elapsed_ms = started.elapsed().as_millis() as u64;
                return context;
            }
        };
        let root = match automation.element_from_handle(Handle::from(target.window_handle)) {
            Ok(value) => value,
            Err(error) => {
                context.status = error_status(&error.to_string());
                context.elapsed_ms = started.elapsed().as_millis() as u64;
                return context;
            }
        };
        if root.get_process_id().ok() != Some(target.process_id) {
            context.status = CaptureStatus::Failed;
            context.elapsed_ms = started.elapsed().as_millis() as u64;
            return context;
        }
        context.window_title = root.get_name().ok().and_then(|value| {
            let (value, truncated) = truncate_chars(&normalize_text(&value), FIELD_CHAR_LIMIT);
            context.truncated |= truncated;
            (!value.is_empty()).then_some(value)
        });
        if context.app_name.is_empty() {
            context.app_name = context
                .window_title
                .clone()
                .unwrap_or_else(|| format!("PID {}", target.process_id));
        }

        let walker = match automation.get_content_view_walker() {
            Ok(value) => value,
            Err(error) => {
                context.status = error_status(&error.to_string());
                context.elapsed_ms = started.elapsed().as_millis() as u64;
                return context;
            }
        };
        let focus = automation
            .get_focused_element()
            .ok()
            .filter(|element| element.get_process_id().ok() == Some(target.process_id));
        let focus_is_sensitive = focus.as_ref().is_some_and(is_sensitive);

        let mut collector = Collector::new(context, options);
        if let Some(focus) = focus.as_ref().filter(|_| !focus_is_sensitive) {
            collector.collect_focus(focus, &walker);
        }
        if !focus_is_sensitive && !collector.is_expired() {
            collector.collect_document(focus.as_ref(), &root, &walker);
        }
        if !collector.is_expired() {
            collector.collect_visible_tree(&root, &walker);
        }
        let mut context = collector.finish();
        enforce_total_budget(&mut context, options.max_chars);
        context.elapsed_ms = started.elapsed().as_millis() as u64;
        context.status = if expired(options.deadline) {
            CaptureStatus::TimedOut
        } else if !context.has_content() {
            CaptureStatus::Empty
        } else {
            CaptureStatus::Captured
        };
        context
    }
}

struct Collector {
    context: CapturedActiveAppContext,
    options: CaptureOptions,
    seen: HashSet<String>,
}

impl Collector {
    fn new(context: CapturedActiveAppContext, options: CaptureOptions) -> Self {
        let mut seen = HashSet::new();
        for value in [
            context.app_name.as_str(),
            context.window_title.as_deref().unwrap_or(""),
        ] {
            if !value.is_empty() {
                seen.insert(value.to_lowercase());
            }
        }
        Self {
            context,
            options,
            seen,
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

    fn collect_focus(&mut self, focus: &UIElement, walker: &UITreeWalker) {
        if !self.visit() || is_sensitive(focus) {
            return;
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
        if self.context.focused_text.is_none() {
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
        self.collect_element_metadata(focus);

        let mut current = focus.clone();
        for _ in 0..MAX_ANCESTORS {
            if self.is_expired() {
                break;
            }
            let Ok(parent) = walker.get_parent(&current) else {
                break;
            };
            if parent.get_process_id().ok() != Some(self.context.process_id) {
                break;
            }
            if !is_sensitive(&parent) {
                self.collect_element_metadata(&parent);
                if let Ok(previous) = walker.get_previous_sibling(&current) {
                    self.collect_element_metadata(&previous);
                }
                if let Ok(next) = walker.get_next_sibling(&current) {
                    self.collect_element_metadata(&next);
                }
            }
            current = parent;
        }
    }

    fn collect_element_metadata(&mut self, element: &UIElement) {
        if !self.visit() || is_sensitive(element) || element.is_offscreen().unwrap_or(false) {
            return;
        }
        self.push_nearby(element.get_name().ok());
        if self.is_expired() {
            return;
        }
        self.push_nearby(element.get_help_text().ok());
        if self.is_expired() {
            return;
        }
        self.push_nearby(element.get_item_type().ok());
        if self.is_expired() {
            return;
        }
        self.push_nearby(element.get_localized_control_type().ok());
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

    fn collect_document(
        &mut self,
        focus: Option<&UIElement>,
        root: &UIElement,
        walker: &UITreeWalker,
    ) {
        let mut candidates = Vec::new();
        if let Some(focus) = focus {
            let mut current = focus.clone();
            for _ in 0..MAX_ANCESTORS {
                if self.is_expired() {
                    break;
                }
                let Ok(parent) = walker.get_parent(&current) else {
                    break;
                };
                if parent.get_process_id().ok() != Some(self.context.process_id) {
                    break;
                }
                if parent.get_control_type().ok() == Some(ControlType::Document) {
                    candidates.push(parent.clone());
                    break;
                }
                current = parent;
            }
        }
        if candidates.is_empty() {
            let mut queue = VecDeque::from([root.clone()]);
            while let Some(element) = queue.pop_front() {
                if !self.visit() {
                    break;
                }
                let sensitive = is_sensitive(&element);
                if !sensitive
                    && !element.is_offscreen().unwrap_or(false)
                    && element.get_control_type().ok() == Some(ControlType::Document)
                {
                    candidates.push(element.clone());
                    break;
                }
                if sensitive {
                    continue;
                }
                if let Ok(child) = walker.get_first_child(&element) {
                    let mut current = child.clone();
                    queue.push_back(child);
                    while let Ok(next) = walker.get_next_sibling(&current) {
                        queue.push_back(next.clone());
                        current = next;
                        if queue.len() + self.context.visited_nodes >= self.options.max_nodes {
                            break;
                        }
                    }
                }
            }
        }
        for candidate in candidates {
            if self.is_expired() || is_sensitive(&candidate) {
                break;
            }
            let Ok(pattern) = candidate.get_pattern::<UITextPattern>() else {
                continue;
            };
            let ranges = pattern.get_visible_ranges().unwrap_or_default();
            if ranges.is_empty() {
                if let Ok(range) = pattern.get_document_range() {
                    self.push_document_range(&range);
                }
            } else {
                for range in ranges {
                    self.push_document_range(&range);
                    if self.is_expired() {
                        break;
                    }
                }
            }
        }
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

    fn collect_visible_tree(&mut self, root: &UIElement, walker: &UITreeWalker) {
        let mut queue = VecDeque::from([root.clone()]);
        while let Some(element) = queue.pop_front() {
            if !self.visit() {
                break;
            }
            let sensitive = is_sensitive(&element);
            if !sensitive && !element.is_offscreen().unwrap_or(false) {
                if let Ok(name) = element.get_name() {
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
            if let Ok(child) = walker.get_first_child(&element) {
                let mut current = child.clone();
                queue.push_back(child);
                while let Ok(next) = walker.get_next_sibling(&current) {
                    queue.push_back(next.clone());
                    current = next;
                    if queue.len() + self.context.visited_nodes >= self.options.max_nodes {
                        break;
                    }
                }
            }
        }
    }

    fn finish(mut self) -> CapturedActiveAppContext {
        self.context.truncated |= self.context.visited_nodes >= self.options.max_nodes;
        self.context
    }
}

fn is_sensitive(element: &UIElement) -> bool {
    element.is_password().unwrap_or(true)
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
}
