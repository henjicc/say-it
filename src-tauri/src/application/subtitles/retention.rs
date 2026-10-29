use super::{SubtitlePrefs, TranslationDocument, MAX_TEXT_CHARS};
use std::collections::{BTreeMap, BTreeSet};

// 多留一个字符才能保留旧显示逻辑“仅截断时去掉开头空白”的行为。
const STABLE_TAIL_CHARS: usize = MAX_TEXT_CHARS + 1;

fn prune_group(group: &mut Vec<u64>, stable: &BTreeMap<u64, usize>) {
    group.retain(|seq| stable.get(seq) != Some(&0));
    let mut chars = 0;
    for index in (0..group.len()).rev() {
        chars += stable.get(&group[index]).copied().unwrap_or(0);
        if chars >= STABLE_TAIL_CHARS {
            group.drain(..index);
            break;
        }
    }
}

impl TranslationDocument {
    pub(super) fn finish(&mut self, seq: u64) {
        if self.values.contains_key(&seq) {
            self.completed.insert(seq);
            self.prune_completed();
        }
    }

    pub(super) fn prune_completed(&mut self) {
        // 只有 done 的文本才提供稳定下界。流式结果可以缩短，不能据此丢弃历史。
        let stable: BTreeMap<_, _> = self
            .completed
            .iter()
            .filter_map(|seq| {
                self.values
                    .get(seq)
                    .map(|text| (*seq, text.chars().count()))
            })
            .collect();
        prune_group(&mut self.current_group, &stable);
        for group in self
            .committed_groups
            .iter_mut()
            .chain(&mut self.replace_groups)
        {
            prune_group(group, &stable);
        }
        // 替换模式无需保留已经确定为空的组；滚动历史的 12 组位置继续保留。
        self.replace_groups.retain(|group| !group.is_empty());
        let mut chars = 0;
        for index in (0..self.replace_groups.len()).rev() {
            chars += self.replace_groups[index]
                .iter()
                .map(|seq| stable.get(seq).copied().unwrap_or(0))
                .sum::<usize>();
            if chars >= STABLE_TAIL_CHARS {
                self.replace_groups.drain(..index);
                break;
            }
        }
        let retained: BTreeSet<_> = self
            .current_group
            .iter()
            .chain(self.committed_groups.iter().flatten())
            .chain(self.replace_groups.iter().flatten())
            .copied()
            .collect();
        self.values.retain(|seq, _| retained.contains(seq));
        self.completed.retain(|seq| retained.contains(seq));
    }

    pub(super) fn render_tail(&self, prefs: &SubtitlePrefs) -> String {
        let replace = prefs.mode == "replace";
        let groups = if replace {
            &self.replace_groups
        } else {
            &self.committed_groups
        };
        let mut reversed = Vec::with_capacity(STABLE_TAIL_CHARS);
        let mut lines = 0;
        for group in groups
            .iter()
            .chain(std::iter::once(&self.current_group))
            .rev()
        {
            let mut chars = group
                .iter()
                .rev()
                .filter_map(|seq| self.values.get(seq))
                .flat_map(|text| text.chars().rev())
                .peekable();
            if chars.peek().is_none() {
                continue;
            }
            if lines > 0 {
                reversed.push(if replace { ' ' } else { '\n' });
            }
            reversed.extend(chars.take(STABLE_TAIL_CHARS - reversed.len()));
            lines += 1;
            if reversed.len() == STABLE_TAIL_CHARS || (!replace && lines >= prefs.line_count.max(1))
            {
                break;
            }
        }
        let clipped = reversed.len() > MAX_TEXT_CHARS;
        reversed.truncate(MAX_TEXT_CHARS);
        let text: String = reversed.into_iter().rev().collect();
        if clipped {
            text.trim_start().into()
        } else {
            text
        }
    }
}

#[cfg(test)]
mod tests;
