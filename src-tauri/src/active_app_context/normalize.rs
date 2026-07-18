use super::model::{CapturedActiveAppContext, ABSOLUTE_MAX_CHARS};
use std::collections::HashSet;

// 实现上提到 `crate::ocr`（系统 OCR 引擎与本模块共用同一套空白折叠规则）。
pub(crate) use crate::ocr::normalize_text;

pub(crate) fn truncate_chars(value: &str, limit: usize) -> (String, bool) {
    let limit = limit.min(ABSOLUTE_MAX_CHARS);
    let mut chars = value.chars();
    let output: String = chars.by_ref().take(limit).collect();
    (output, chars.next().is_some())
}

fn take_option(
    value: &mut Option<String>,
    remaining: &mut usize,
    truncated: &mut bool,
    seen: &mut HashSet<String>,
) {
    let Some(current) = value.take() else { return };
    if *remaining == 0 {
        *truncated = true;
        return;
    }
    let normalized = normalize_text(&current);
    if normalized.is_empty() || !seen.insert(normalized.to_lowercase()) {
        return;
    }
    let (current, was_truncated) = truncate_chars(&normalized, *remaining);
    *remaining = remaining.saturating_sub(current.chars().count());
    *truncated |= was_truncated;
    *value = (!current.is_empty()).then_some(current);
}

fn take_list(
    values: &mut Vec<String>,
    remaining: &mut usize,
    truncated: &mut bool,
    seen: &mut HashSet<String>,
) {
    let mut output = Vec::new();
    for value in std::mem::take(values) {
        if *remaining == 0 {
            *truncated = true;
            break;
        }
        let normalized = normalize_text(&value);
        if normalized.is_empty() || !seen.insert(normalized.to_lowercase()) {
            continue;
        }
        let (value, was_truncated) = truncate_chars(&normalized, *remaining);
        *remaining = remaining.saturating_sub(value.chars().count());
        *truncated |= was_truncated;
        if !value.is_empty() {
            output.push(value);
        }
    }
    *values = output;
}

pub(crate) fn enforce_total_budget(context: &mut CapturedActiveAppContext, max_chars: usize) {
    let mut remaining = max_chars.min(ABSOLUTE_MAX_CHARS);
    let mut truncated = context.truncated;
    let mut seen = HashSet::new();

    take_option(
        &mut context.selected_text,
        &mut remaining,
        &mut truncated,
        &mut seen,
    );
    take_option(
        &mut context.focused_text,
        &mut remaining,
        &mut truncated,
        &mut seen,
    );
    take_option(
        &mut context.caret_context,
        &mut remaining,
        &mut truncated,
        &mut seen,
    );
    take_list(
        &mut context.visible_text,
        &mut remaining,
        &mut truncated,
        &mut seen,
    );
    take_list(
        &mut context.document_text,
        &mut remaining,
        &mut truncated,
        &mut seen,
    );
    take_list(
        &mut context.ocr_text,
        &mut remaining,
        &mut truncated,
        &mut seen,
    );
    context.truncated = truncated;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_whitespace() {
        assert_eq!(normalize_text("  Hello\n world  "), "Hello world");
    }

    #[test]
    fn budget_follows_context_priority() {
        let mut context = CapturedActiveAppContext {
            selected_text: Some("123".into()),
            focused_text: Some("45".into()),
            document_text: vec!["67890".into()],
            ocr_text: vec!["abc".into()],
            ..Default::default()
        };
        enforce_total_budget(&mut context, 7);
        assert_eq!(context.selected_text.as_deref(), Some("123"));
        assert_eq!(context.focused_text.as_deref(), Some("45"));
        assert_eq!(context.document_text, vec!["67"]);
        assert!(context.ocr_text.is_empty());
        assert!(context.truncated);
    }

    #[test]
    fn duplicate_text_is_kept_only_at_its_highest_priority_source() {
        let mut context = CapturedActiveAppContext {
            selected_text: Some("Same".into()),
            focused_text: Some("same".into()),
            document_text: vec!["Same".into(), "Different".into()],
            ..Default::default()
        };
        enforce_total_budget(&mut context, 100);
        assert_eq!(context.selected_text.as_deref(), Some("Same"));
        assert!(context.focused_text.is_none());
        assert_eq!(context.document_text, vec!["Different"]);
    }
}
