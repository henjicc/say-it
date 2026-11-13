//! 把 sherpa-onnx 的逐 token 时间整理成字幕与文稿对齐要的逐字（英文逐词）时间。
use super::LocalWord;

/// 一个字最多持续这么久；下一个字之前的停顿不算进上一个字。
const MAX_WORD_MS: u64 = 1000;

fn is_punctuation(c: char) -> bool {
    c.is_ascii_punctuation() || "，。！？、；：“”‘’（）《》【】…—·".contains(c)
}

fn is_latin(c: char) -> bool {
    c.is_ascii_alphanumeric()
}

/// `tokens` 是 token 与它在句段内的起始秒数。中文一个 token 一个字；英文按 `▁` 开头的
/// 子词拼成词；标点挂到前一个字上，与云端识别结果的格式一致。时间换算成整段音频里的毫秒。
pub(super) fn words_from_tokens(tokens: &[(String, f32)], begin_ms: u64, end_ms: u64) -> Vec<LocalWord> {
    let mut words: Vec<LocalWord> = Vec::new();
    for (token, start) in tokens {
        // 语言、情感等标签不是文字。
        if token.starts_with("<|") {
            continue;
        }
        let starts_word = token.starts_with('▁');
        let piece = token.trim_start_matches('▁');
        if piece.is_empty() {
            continue;
        }
        if piece.chars().all(is_punctuation) {
            if let Some(last) = words.last_mut() {
                last.punctuation.get_or_insert_with(String::new).push_str(piece);
            }
            continue;
        }
        let at = (begin_ms + (start.max(0.0) * 1000.0).round() as u64).min(end_ms);
        let continues_word = !starts_word
            && words.last().is_some_and(|last| {
                last.punctuation.is_none()
                    && last.text.chars().last().is_some_and(is_latin)
                    && piece.chars().next().is_some_and(is_latin)
            });
        if continues_word {
            words.last_mut().unwrap().text.push_str(piece);
            continue;
        }
        if let Some(last) = words.last_mut() {
            last.end_ms = at.clamp(last.begin_ms, last.begin_ms + MAX_WORD_MS);
        }
        words.push(LocalWord { text: piece.to_string(), begin_ms: at, end_ms, punctuation: None });
    }
    if let Some(last) = words.last_mut() {
        last.end_ms = end_ms.clamp(last.begin_ms, last.begin_ms + MAX_WORD_MS);
    }
    words
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(items: &[(&str, f32)]) -> Vec<(String, f32)> {
        items.iter().map(|(text, at)| (text.to_string(), *at)).collect()
    }

    #[test]
    fn chinese_characters_get_their_own_times_and_punctuation_attaches_to_the_previous_one() {
        let words = words_from_tokens(
            &tokens(&[("<|zh|>", 0.0), ("今", 0.3), ("天", 0.5), ("，", 0.6), ("好", 3.0), ("。", 3.2)]),
            10_000,
            14_000,
        );
        let summary: Vec<_> = words
            .iter()
            .map(|w| (w.text.as_str(), w.begin_ms, w.end_ms, w.punctuation.as_deref()))
            .collect();
        assert_eq!(
            summary,
            [
                ("今", 10_300, 10_500, None),
                // 下一个字在 2.5 秒后，中间的停顿不算进这个字。
                ("天", 10_500, 11_500, Some("，")),
                ("好", 13_000, 14_000, Some("。")),
            ]
        );
    }

    #[test]
    fn english_subwords_join_into_words() {
        let words = words_from_tokens(
            &tokens(&[("▁hel", 0.0), ("lo", 0.2), ("▁world", 0.5), ("!", 0.9), ("▁ok", 1.0)]),
            0,
            1_500,
        );
        let texts: Vec<_> = words.iter().map(|w| (w.text.as_str(), w.begin_ms, w.punctuation.as_deref())).collect();
        assert_eq!(texts, [("hello", 0, None), ("world", 500, Some("!")), ("ok", 1_000, None)]);
    }
}
