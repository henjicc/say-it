//! 整理听写时，输出的大部分 token 是照抄来的：要么抄原文，要么抄同一段话上一版的整理结果。
//! 这里猜「接下来会抄哪一段」，由模型一次前向批量验证，猜中的 token 不用逐个生成。

/// 用来对齐的输出末尾最长 token 数。越长越不会对到别处，但改动多的地方更难对上。
const MAX_MATCH: usize = 4;

/// 一段可以照抄的参考文本，记着上一次对上的位置。
pub(crate) struct Reference<'a> {
    tokens: &'a [i32],
    /// 上一次对齐后，参考里接下来该抄的位置。同一片段在参考里出现多次时，选离它最近的。
    cursor: usize,
}

/// 输出末尾在参考里对上的地方。
struct Alignment {
    /// 对上的 token 数；越多越可信。
    matched: usize,
    /// 草稿从参考的这个位置开始。
    continue_at: usize,
}

impl<'a> Reference<'a> {
    pub(crate) fn new(tokens: &'a [i32]) -> Self {
        Self { tokens, cursor: 0 }
    }

    /// 输出末尾（最多 `MAX_MATCH` 个 token）在参考里离游标最近的出现处。
    fn align(&self, output: &[i32]) -> Option<Alignment> {
        for matched in (1..=MAX_MATCH.min(output.len())).rev() {
            let tail = &output[output.len() - matched..];
            let best = self
                .tokens
                .windows(matched)
                .enumerate()
                .filter(|(_, window)| *window == tail)
                .map(|(start, _)| start + matched)
                .min_by_key(|end| end.abs_diff(self.cursor));
            if let Some(continue_at) = best {
                return Some(Alignment {
                    matched,
                    continue_at,
                });
            }
        }
        None
    }

    /// 给出接在 `output` 之后的草稿，并把游标移到对上的地方。
    /// 输出末尾对不上（模型刚插了参考里没有的标点、编号或改写）时不猜：实测这时下一个 token
    /// 几乎总是继续插入，跳过插入去对齐的草稿基本猜不中。
    fn propose(&mut self, output: &[i32]) -> Option<(usize, &'a [i32])> {
        let alignment = self.align(output)?;
        self.cursor = alignment.continue_at;
        let rest = &self.tokens[alignment.continue_at..];
        (!rest.is_empty()).then_some((alignment.matched, rest))
    }
}

pub(crate) struct Draft<'a> {
    pub(crate) tokens: &'a [i32],
    /// 输出末尾与参考对上的 token 数。
    pub(crate) matched: usize,
    /// 草稿来自上一版结果：它连标点都对，往往能连中很长一段。
    pub(crate) from_previous: bool,
}

/// 两个参考各自对齐，取对上得更多的那个出草稿；一样多时优先上一版结果。都对不上时不猜：
/// 白猜的一次前向比只算一个 token 贵得多。
pub(crate) fn propose<'a>(
    previous: &mut Reference<'a>,
    source: &mut Reference<'a>,
    output: &[i32],
) -> Option<Draft<'a>> {
    let from_previous = previous.propose(output);
    let from_source = source.propose(output);
    match (from_previous, from_source) {
        (Some((matched, tokens)), Some((other, _))) if matched >= other => Some(Draft {
            tokens,
            matched,
            from_previous: true,
        }),
        (_, Some((matched, tokens))) => Some(Draft {
            tokens,
            matched,
            from_previous: false,
        }),
        (Some((matched, tokens)), None) => Some(Draft {
            tokens,
            matched,
            from_previous: true,
        }),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn realigns_after_skipped_filler_and_waits_out_inserted_punctuation() {
        // 原文：1 2 [9 9] 3 4 5，输出：1 2 ，3（删掉 9 9，插入标点 100）
        let source = [1, 2, 9, 9, 3, 4, 5];
        let mut reference = Reference::new(&source);
        assert_eq!(reference.propose(&[1, 2]).unwrap().1, &[9, 9, 3, 4, 5]);
        assert!(reference.propose(&[1, 2, 100]).is_none(), "刚插入标点时不猜");
        assert_eq!(reference.propose(&[1, 2, 100, 3]).unwrap().1, &[4, 5]);
    }

    #[test]
    fn repeated_tokens_align_to_the_occurrence_nearest_the_cursor() {
        let source = [7, 1, 7, 2, 7, 3];
        let mut reference = Reference::new(&source);
        assert_eq!(reference.propose(&[7, 1]).unwrap().1, &[7, 2, 7, 3]);
        assert_eq!(reference.propose(&[7, 1, 7, 2, 7]).unwrap().1, &[3]);
    }

    #[test]
    fn previous_output_is_preferred_until_the_output_leaves_it() {
        let previous_tokens = [1, 100, 2];
        let source_tokens = [1, 2, 3, 4];
        let mut previous = Reference::new(&previous_tokens);
        let mut source = Reference::new(&source_tokens);
        let draft = propose(&mut previous, &mut source, &[1]).unwrap();
        assert!(draft.from_previous);
        assert_eq!(draft.tokens, &[100, 2]);
        // 上一版结果用完，剩下的新内容只能抄原文。
        let draft = propose(&mut previous, &mut source, &[1, 100, 2]).unwrap();
        assert!(!draft.from_previous);
        assert_eq!(draft.tokens, &[3, 4]);
    }

    #[test]
    fn no_draft_when_neither_reference_matches() {
        let mut previous = Reference::new(&[1, 2]);
        let mut source = Reference::new(&[3, 4]);
        assert!(propose(&mut previous, &mut source, &[8, 9]).is_none());
    }
}
