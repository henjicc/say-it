//! 给“这个”“这里”补上所指的内容：口述里的指代词说出那一刻前后，用户正好选中了文字或用鼠标
//! 指着某处时，在整理结果里同一个指代词后面加上「（所指的文字）」。原话一字不删，只补注。
//! 对面看不到用户的屏幕时（对 AI 说话、语音问答）才有用，是否启用由调用方决定。
//! 补错了会改坏用户要发出的话，所以宁可少补：只用读得准的指向（按坐标读到的原文、控件名称、选中的
//! 文字，不用截图识别的），时间要贴得很近，指代词也只认明确当代词用的。
use super::dynamic::{DynamicEntry, DynamicKind};

/// 指代词，长的排前面：先认「这一段」再认「这」开头的短词。
const DEICTICS: &[&str] = &[
    "这一段", "这一行", "这一句", "这个", "那个", "这里", "那里", "这儿", "那儿", "这块", "这段", "这行",
    "这句", "这张",
];
/// 跟在指代词后面、本身说不清是哪一个的名词：「这个按钮」仍要补注，所以补在名词后面。
const GENERIC_NOUNS: &[&str] = &[
    "按钮", "图标", "选项", "菜单", "链接", "报错", "错误", "函数", "变量", "参数", "字段", "设置", "页面",
    "窗口", "文件", "代码", "表格", "数字", "颜色", "图片", "地方", "位置", "东西", "内容", "部分", "单词",
    "句子", "段落", "词", "字", "图", "段", "句", "行", "块", "处",
];
/// 指代词后面紧跟这些字时，它是单独当代词用的（「这个是什么」「这里的」「这个和那个」）。「这个怎么回事」
/// 「这个不应该」这类说的多半是前面提过的事，不算。
const PRONOUN_FOLLOWERS: &str = "是呢吗啊吧的和跟或";

/// 口述说出后，识别文字大约晚这么久才出现。
const RECOGNITION_LAG_MS: i64 = 600;
/// 操作发生在说出指代词之前这么久以内、之后这么久以内，才算指的是它。
const POINT_BEFORE_MS: i64 = 1_000;
const POINT_AFTER_MS: i64 = 500;
/// 人多是先指再说：说出之后的操作，时间差按这个倍数算，同样远时优先取之前的。
const AFTER_PENALTY: i64 = 2;
/// 补注最多这么多字，超出以 … 结尾。
const NOTE_CHARS: usize = 24;

/// 文字里的一个指代词：`at` 是它在文字里的字符位置，`insert` 是补注要插入的字符位置。
#[derive(Debug, PartialEq)]
struct Reference {
    at: usize,
    insert: usize,
}

fn references(text: &str) -> Vec<Reference> {
    let chars: Vec<char> = text.chars().collect();
    let starts_with = |index: usize, word: &str| {
        let word: Vec<char> = word.chars().collect();
        chars.get(index..index + word.len()) == Some(&word[..])
    };
    let punctuation = |value: Option<&char>| value.is_none_or(|c| !c.is_alphanumeric());
    let mut found = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        let Some(word) = DEICTICS.iter().find(|word| starts_with(index, word)) else {
            index += 1;
            continue;
        };
        let after = index + word.chars().count();
        index = after;
        // 「这个这个」只算后一个。
        if starts_with(after, word) {
            continue;
        }
        let before = after.checked_sub(word.chars().count() + 1).and_then(|i| chars.get(i));
        // 句首的「那个，」「这个，」是口头禅，不是指东西。
        if punctuation(before) && chars.get(after).is_some_and(|c| !c.is_alphanumeric()) {
            continue;
        }
        if let Some(noun) = GENERIC_NOUNS.iter().find(|noun| starts_with(after, noun)) {
            found.push(Reference { at: after - word.chars().count(), insert: after + noun.chars().count() });
        } else if punctuation(chars.get(after)) || chars.get(after).is_some_and(|c| PRONOUN_FOLLOWERS.contains(*c)) {
            found.push(Reference { at: after - word.chars().count(), insert: after });
        }
    }
    found
}

/// 识别文字里第 `index` 个字大约是开口后第几毫秒说出的。`spoken` 是一串（时间，当时已识别的字数）。
fn spoken_at(spoken: &[(u64, usize)], index: usize) -> Option<i64> {
    spoken
        .iter()
        .find(|(_, chars)| *chars > index)
        .or(spoken.last())
        .map(|(at, _)| *at as i64 - RECOGNITION_LAG_MS)
}

fn note_for(entries: &[DynamicEntry], at: i64) -> Option<String> {
    entries
        .iter()
        .filter_map(|entry| {
            let text = match &entry.kind {
                DynamicKind::Selection { text } | DynamicKind::Pointer { text, exact: true } => text,
                DynamicKind::Pointer { exact: false, .. } | DynamicKind::Window { .. } => return None,
            };
            let gap = entry.at_ms as i64 - at;
            let score = if gap > 0 { gap * AFTER_PENALTY } else { -gap };
            (-POINT_BEFORE_MS..=POINT_AFTER_MS).contains(&gap).then_some((score, text))
        })
        .min_by_key(|(gap, _)| *gap)
        .map(|(_, text)| {
            let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
            let mut note: String = text.chars().take(NOTE_CHARS).collect();
            if note.chars().count() < text.chars().count() {
                note.push('…');
            }
            note
        })
}

/// `raw` 是识别原文，`output` 是整理后要输出的文字。两边认出的指代词个数不同时（整理删掉或改写了
/// 一些）对不上是哪一个，原样返回。
pub(crate) fn annotate_references(
    raw: &str,
    output: &str,
    entries: &[DynamicEntry],
    spoken: &[(u64, usize)],
) -> String {
    let in_raw = references(raw);
    let in_output = references(output);
    if in_raw.is_empty() || in_raw.len() != in_output.len() {
        return output.to_string();
    }
    let mut notes: Vec<(usize, String)> = in_raw
        .iter()
        .zip(&in_output)
        .filter_map(|(spoken_ref, output_ref)| {
            let note = note_for(entries, spoken_at(spoken, spoken_ref.at)?)?;
            // 用户已经把所指的内容说出来了，不用再补。
            (!output.contains(&note)).then_some((output_ref.insert, note))
        })
        .collect();
    let mut chars: Vec<char> = output.chars().collect();
    notes.sort_by(|a, b| b.0.cmp(&a.0));
    for (insert, note) in notes {
        chars.splice(insert..insert, format!("（{note}）").chars());
    }
    chars.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pointer(at_ms: u64, text: &str) -> DynamicEntry {
        DynamicEntry { at_ms, kind: DynamicKind::Pointer { text: text.into(), exact: true } }
    }

    #[test]
    fn only_deictic_uses_are_references() {
        let positions = |text: &str| references(text).into_iter().map(|r| (r.at, r.insert)).collect::<Vec<_>>();
        // 单独当代词、后面跟泛称名词的算；后面跟着具体名词、句首口头禅不算。
        assert_eq!(positions("这个是什么"), vec![(0, 2)]);
        assert_eq!(positions("点一下这个按钮"), vec![(3, 7)]);
        assert_eq!(positions("这个方案还行"), vec![]);
        assert_eq!(positions("那个，我想说"), vec![]);
        assert_eq!(positions("看看这里的报错"), vec![(2, 4)]);
        assert_eq!(positions("就是这个这个"), vec![(4, 6)]);
        assert_eq!(positions("改成这样"), vec![]);
    }

    #[test]
    fn notes_the_thing_pointed_at_when_the_word_was_said() {
        // 「这个」在第 4 个字，识别文字到第 6 个字时是 3.0 秒；鼠标 2.6 秒指着「导出设置」。
        let spoken = [(1_000, 3), (3_000, 6), (4_000, 12)];
        let entries = [pointer(800, "早先指过的"), pointer(2_600, "导出设置")];
        assert_eq!(
            annotate_references("我想问一下这个按钮怎么用", "我想问一下这个按钮怎么用？", &entries, &spoken),
            "我想问一下这个按钮（导出设置）怎么用？"
        );
        // 两个指代词各配各自时间的那次指向。
        let spoken = [(1_500, 2), (4_500, 12)];
        let entries = [pointer(900, "方案甲"), pointer(4_000, "方案乙")];
        assert_eq!(
            annotate_references("这个和那个，哪个好", "这个和那个，哪个好？", &entries, &spoken),
            "这个（方案甲）和那个（方案乙），哪个好？"
        );
    }

    /// 实测补错过的句子：截图识别出的乱码、前面提过的事、复数指代，都不该补注。
    #[test]
    fn does_not_note_from_ocr_or_non_pointing_phrases() {
        let spoken = [(1_000, 4), (2_000, 40)];
        let ocr = DynamicEntry { at_ms: 1_000, kind: DynamicKind::Pointer { text: "ane.".into(), exact: false } };
        let near = |text: &str| [pointer(1_000, text)];
        assert_eq!(
            annotate_references("箭头所指的这个位置", "箭头所指的这个位置", &[ocr], &spoken),
            "箭头所指的这个位置"
        );
        for sentence in ["DeepSeek 这个不应该啊", "不要那些东西", "你这个怎么回事啊"] {
            assert_eq!(annotate_references(sentence, sentence, &near("提示词"), &spoken), sentence);
        }
        // 选中的文字读得准，照样补。
        let selected = [DynamicEntry { at_ms: 500, kind: DynamicKind::Selection { text: "导出设置".into() } }];
        assert_eq!(annotate_references("这个是什么", "这个是什么？", &selected, &spoken), "这个（导出设置）是什么？");
    }

    #[test]
    fn leaves_the_output_alone_when_it_cannot_tell() {
        let spoken = [(2_000, 10)];
        let entries = [pointer(1_500, "导出设置")];
        // 整理删掉了一个「这个」，对不上是哪一个。
        assert_eq!(annotate_references("这个这个是什么那个呢", "这个是什么？", &entries, &spoken), "这个是什么？");
        // 附近没有指向或选中。
        assert_eq!(annotate_references("这个是什么", "这个是什么？", &[pointer(9_000, "远")], &spoken), "这个是什么？");
        // 已经说出了所指的内容。
        assert_eq!(annotate_references("这个导出设置是什么", "这个导出设置是什么？", &entries, &spoken), "这个导出设置是什么？");
        // 太长的所指截断。
        let long = "字".repeat(40);
        assert_eq!(
            annotate_references("这个是什么", "这个是什么？", &[pointer(1_500, &long)], &spoken),
            format!("这个（{}…）是什么？", "字".repeat(NOTE_CHARS))
        );
    }
}
