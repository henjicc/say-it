/// 对齐输入的词级时间戳拆出的最小对齐单元（CJK 单字 / 拉丁连串 / 数字单字符）。
pub(super) struct AsrToken {
    pub(super) canon: String,
    pub(super) begin_ms: u64,
    pub(super) end_ms: u64,
    /// 所属词在输入 words 中的原始下标。
    pub(super) word_index: usize,
}

/// 把 ASR 词按时间排序后拆成 token，多 token 词内部按字符数线性内插时间。
pub(super) fn build_asr_tokens(words: &[super::AlignWord]) -> Vec<AsrToken> {
    let mut sorted: Vec<(usize, &super::AlignWord)> = words.iter().enumerate().collect();
    sorted.sort_by_key(|(_, w)| w.begin_time);
    let mut tokens = Vec::new();
    for (word_index, word) in sorted {
        let parts = tokenize_text(&word.text);
        if parts.is_empty() {
            continue;
        }
        let begin = word.begin_time;
        let end = word.end_time.max(begin);
        let span = end - begin;
        let total_chars: u64 = parts.iter().map(|p| p.chars().count() as u64).sum();
        let mut acc = 0u64;
        for part in parts {
            let chars = part.chars().count() as u64;
            let b = begin + span * acc / total_chars;
            acc += chars;
            let e = begin + span * acc / total_chars;
            tokens.push(AsrToken {
                canon: part,
                begin_ms: b,
                end_ms: e,
                word_index,
            });
        }
    }
    tokens
}

/// 规整并切分文本：CJK 单字一 token、连续拉丁字母一 token、数字逐字符一 token，
/// 标点/空白/符号只作分隔。
fn tokenize_text(text: &str) -> Vec<String> {
    tokenize_with_spans(text).into_iter().map(|(t, _, _)| t).collect()
}

/// 同 `tokenize_text`，额外返回每个 token 在原始字符串中的字节范围
/// （用于“识别修正”结果按保留片段直接切原文子串，保留标点/间距原样）。
pub(super) fn tokenize_with_spans(text: &str) -> Vec<(String, usize, usize)> {
    let mut tokens = Vec::new();
    let mut latin = String::new();
    let mut latin_start = 0usize;
    for (byte_idx, raw) in text.char_indices() {
        let c = canonical_char(raw);
        if c.is_ascii_digit() || is_cjk(c) {
            flush_latin_spans(&mut latin, latin_start, byte_idx, &mut tokens);
            tokens.push((c.to_string(), byte_idx, byte_idx + raw.len_utf8()));
        } else if c.is_alphabetic() {
            if latin.is_empty() {
                latin_start = byte_idx;
            }
            latin.push(c);
        } else {
            flush_latin_spans(&mut latin, latin_start, byte_idx, &mut tokens);
        }
    }
    flush_latin_spans(&mut latin, latin_start, text.len(), &mut tokens);
    tokens
}

fn flush_latin_spans(latin: &mut String, start: usize, end: usize, tokens: &mut Vec<(String, usize, usize)>) {
    if !latin.is_empty() {
        tokens.push((std::mem::take(latin), start, end));
    }
}

fn canonical_char(raw: char) -> char {
    // 全角 ASCII 区与全角空格折半角（NFKC 的简化子集，覆盖中文文本的常见差异）
    let c = match raw as u32 {
        0xFF01..=0xFF5E => char::from_u32(raw as u32 - 0xFEE0).unwrap_or(raw),
        0x3000 => ' ',
        _ => raw,
    };
    // 中文数字字符与阿拉伯数字互认（两侧同规则），解决“2024”vs“二零二四”类读法差异
    let c = match c {
        '〇' | '零' => '0',
        '一' => '1',
        '二' | '两' => '2',
        '三' => '3',
        '四' => '4',
        '五' => '5',
        '六' => '6',
        '七' => '7',
        '八' => '8',
        '九' => '9',
        other => other,
    };
    c.to_lowercase().next().unwrap_or(c)
}

fn is_cjk(c: char) -> bool {
    matches!(
        c as u32,
        0x3400..=0x4DBF        // 汉字扩展 A
            | 0x4E00..=0x9FFF   // 基本汉字
            | 0xF900..=0xFAFF   // 兼容汉字
            | 0x3040..=0x30FF   // 日文假名
            | 0x31F0..=0x31FF   // 假名扩展
            | 0xAC00..=0xD7AF   // 谚文音节
            | 0x20000..=0x2FA1F // 汉字扩展 B 及以后
    )
}
