//! 整理听写时，输出的大部分 token 是照抄来的：要么抄原文，要么抄同一段话上一版的整理结果。
//! 这里猜「接下来会抄哪一段」，由模型一次前向批量验证，猜中的 token 不用逐个生成。

/// 沿着一段参考文本跟踪输出抄到了哪里。
pub(crate) struct Pointer<'a> {
    reference: &'a [i32],
    next: usize,
    /// 最近几个输出 token 里有多少是从参考文本抄的，越近权重越高。
    confidence: f32,
}

/// 输出跳过参考文本里的一小段（删口头禅、删重复）后还能重新对上的最大跨度。
const SKIP_WINDOW: usize = 12;

impl<'a> Pointer<'a> {
    pub(crate) fn new(reference: &'a [i32]) -> Self {
        Self {
            reference,
            next: 0,
            confidence: 0.0,
        }
    }

    /// 输出多了一个 token：照抄则前进一格，跳过一小段则跟过去，都不是就当作模型自己插入的（如标点）。
    pub(crate) fn follow(&mut self, token: i32) {
        let window = &self.reference[self.next.min(self.reference.len())..];
        let hit = window
            .iter()
            .take(SKIP_WINDOW)
            .position(|candidate| *candidate == token);
        self.confidence *= 0.5;
        if let Some(offset) = hit {
            self.next += offset + 1;
            self.confidence += 1.0;
        }
    }

    fn remaining(&self) -> &'a [i32] {
        &self.reference[self.next.min(self.reference.len())..]
    }
}

pub(crate) struct Draft<'a> {
    pub(crate) tokens: &'a [i32],
    /// 草稿来自上一版结果：它连标点都对，往往能连中很长一段。
    pub(crate) from_previous: bool,
}

/// 两个参考里挑当前跟得更稳的那个出草稿；一样稳时优先上一版结果。
pub(crate) fn propose<'a>(previous: &Pointer<'a>, source: &Pointer<'a>) -> Draft<'a> {
    let from_previous =
        !previous.remaining().is_empty() && previous.confidence >= source.confidence;
    Draft {
        tokens: if from_previous {
            previous.remaining()
        } else {
            source.remaining()
        },
        from_previous,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pointer_skips_deleted_filler_and_ignores_inserted_punctuation() {
        // 原文：1 2 [9 9] 3 4，输出：1 2 ，3 4（删掉 9 9，插入标点 100）
        let source = [1, 2, 9, 9, 3, 4];
        let mut pointer = Pointer::new(&source);
        pointer.follow(1);
        pointer.follow(2);
        pointer.follow(100);
        assert_eq!(pointer.remaining(), &[9, 9, 3, 4]);
        pointer.follow(3);
        assert_eq!(pointer.remaining(), &[4]);
    }

    #[test]
    fn previous_output_is_preferred_until_the_output_leaves_it() {
        let previous_tokens = [1, 100, 2];
        let source_tokens = [1, 2, 3, 4];
        let mut previous = Pointer::new(&previous_tokens);
        let mut source = Pointer::new(&source_tokens);
        let draft = propose(&previous, &source);
        assert!(draft.from_previous);
        assert_eq!(draft.tokens, &[1, 100, 2]);
        for token in [1, 100, 2] {
            previous.follow(token);
            source.follow(token);
        }
        // 上一版结果用完，剩下的新内容只能抄原文。
        let draft = propose(&previous, &source);
        assert!(!draft.from_previous);
        assert_eq!(draft.tokens, &[3, 4]);
    }
}
