use std::collections::HashSet;

use super::model::{CapturedActiveAppContext, ABSOLUTE_MAX_CHARS};

pub(crate) fn normalize_text(value: &str) -> String {
    value
        .split_whitespace()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn truncate_chars(value: &str, limit: usize) -> (String, bool) {
    let limit = limit.min(ABSOLUTE_MAX_CHARS);
    let mut chars = value.chars();
    let output: String = chars.by_ref().take(limit).collect();
    (output, chars.next().is_some())
}

pub(crate) fn push_unique(
    target: &mut Vec<String>,
    seen: &mut HashSet<String>,
    value: &str,
    limit: usize,
) -> bool {
    let normalized = normalize_text(value);
    if normalized.is_empty() {
        return false;
    }
    let (normalized, truncated) = truncate_chars(&normalized, limit);
    let key = normalized.to_lowercase();
    if !seen.insert(key) {
        return truncated;
    }
    target.push(normalized);
    truncated
}

pub(crate) fn enforce_total_budget(context: &mut CapturedActiveAppContext, max_chars: usize) {
    let max_chars = max_chars.min(ABSOLUTE_MAX_CHARS);
    let mut remaining = max_chars;
    let mut truncated = context.truncated;

    fn trim(value: &mut Option<String>, remaining: &mut usize, truncated: &mut bool) {
        let Some(current) = value.take() else { return };
        if *remaining == 0 {
            *truncated = true;
            return;
        }
        let (next, was_truncated) = truncate_chars(&current, *remaining);
        *remaining = remaining.saturating_sub(next.chars().count());
        *truncated |= was_truncated;
        if !next.is_empty() {
            *value = Some(next);
        }
    }

    fn trim_list(values: &mut Vec<String>, remaining: &mut usize, truncated: &mut bool) {
        let mut next = Vec::new();
        for value in std::mem::take(values) {
            if *remaining == 0 {
                *truncated = true;
                break;
            }
            let (value, was_truncated) = truncate_chars(&value, *remaining);
            *remaining = remaining.saturating_sub(value.chars().count());
            *truncated |= was_truncated;
            if !value.is_empty() {
                next.push(value);
            }
        }
        *values = next;
    }

    trim(&mut context.selected_text, &mut remaining, &mut truncated);
    trim(&mut context.focused_text, &mut remaining, &mut truncated);
    trim_list(&mut context.nearby_text, &mut remaining, &mut truncated);
    trim_list(&mut context.document_text, &mut remaining, &mut truncated);
    context.truncated = truncated;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_whitespace_and_deduplicates_case_insensitively() {
        let mut values = Vec::new();
        let mut seen = HashSet::new();
        assert!(!push_unique(
            &mut values,
            &mut seen,
            "  Hello\n world  ",
            100
        ));
        assert!(!push_unique(&mut values, &mut seen, "hello world", 100));
        assert_eq!(values, vec!["Hello world"]);
    }

    #[test]
    fn budget_keeps_high_priority_fields_first() {
        let mut context = CapturedActiveAppContext {
            selected_text: Some("12345".into()),
            focused_text: Some("67890".into()),
            nearby_text: vec!["abc".into()],
            ..Default::default()
        };
        enforce_total_budget(&mut context, 7);
        assert_eq!(context.selected_text.as_deref(), Some("12345"));
        assert_eq!(context.focused_text.as_deref(), Some("67"));
        assert!(context.nearby_text.is_empty());
        assert!(context.truncated);
    }
}
