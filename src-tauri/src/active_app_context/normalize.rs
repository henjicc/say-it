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

pub(crate) fn enforce_total_budget(context: &mut CapturedActiveAppContext, max_chars: usize) {
    let mut remaining = max_chars.min(ABSOLUTE_MAX_CHARS);
    let mut output = Vec::new();
    let mut truncated = context.truncated;
    for value in std::mem::take(&mut context.ocr_text) {
        if remaining == 0 {
            truncated = true;
            break;
        }
        let (value, was_truncated) = truncate_chars(&value, remaining);
        remaining = remaining.saturating_sub(value.chars().count());
        truncated |= was_truncated;
        if !value.is_empty() {
            output.push(value);
        }
    }
    context.ocr_text = output;
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
    fn budget_truncates_ocr_text() {
        let mut context = CapturedActiveAppContext {
            ocr_text: vec!["12345".into(), "67890".into()],
            ..Default::default()
        };
        enforce_total_budget(&mut context, 7);
        assert_eq!(context.ocr_text, vec!["12345", "67"]);
        assert!(context.truncated);
    }
}
