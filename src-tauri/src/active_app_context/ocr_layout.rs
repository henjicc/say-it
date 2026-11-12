//! OCR 文字框的收尾整理：去碎片、只留正文所在的栏、按栏排出阅读顺序、去重、截断。
//! 内置 OCR、插件 OCR 和 macOS Vision 共用，保证不同来源输出的上下文是同一种顺序。
use std::collections::{HashMap, HashSet};

use super::model::OcrTextBlock;

const MAX_BLOCKS: usize = 120;

pub(super) fn finalize_blocks(blocks: Vec<OcrTextBlock>) -> Vec<OcrTextBlock> {
    let mut blocks = main_columns(blocks.into_iter().filter(is_meaningful).collect())
        .into_iter()
        .flat_map(|mut column| {
            sort_blocks_by_reading_order(&mut column);
            column
        })
        .collect::<Vec<_>>();
    let mut seen = HashSet::new();
    blocks.retain(|block| seen.insert(block.text.to_lowercase()));
    blocks.truncate(MAX_BLOCKS);
    blocks
}

/// 文字框里有效字符（汉字、字母、数字）的个数。
fn meaningful_chars(text: &str) -> usize {
    text.chars().filter(|value| value.is_alphanumeric()).count()
}

/// 去掉按钮、图标被误认出的碎片和乱码：有效字符不到 3 个（纯数字除外），
/// 或有效字符占比不到一半。
fn is_meaningful(block: &OcrTextBlock) -> bool {
    let text = block.text.trim();
    let total = text.chars().filter(|value| !value.is_whitespace()).count();
    let useful = meaningful_chars(text);
    let numeric = useful > 0
        && text
            .chars()
            .all(|value| value.is_ascii_digit() || !value.is_alphanumeric());
    (useful >= 3 || numeric) && useful * 2 >= total
}

/// 把文字框分栏，只返回正文所在的栏，按从左到右排列：横向有重叠的文字框归为同一栏，
/// 侧栏、目录、工具栏通常是独立的窄栏。保留字数最多的栏，以及字数和宽度都不少于它一半的栏
/// （比如文档旁边的对话面板）。各栏分开返回，读完一栏再读下一栏，不按行交错。
fn main_columns(blocks: Vec<OcrTextBlock>) -> Vec<Vec<OcrTextBlock>> {
    let mut column: Vec<usize> = (0..blocks.len()).collect();
    fn root(column: &mut [usize], mut index: usize) -> usize {
        while column[index] != index {
            column[index] = column[column[index]];
            index = column[index];
        }
        index
    }
    for left in 0..blocks.len() {
        for right in left + 1..blocks.len() {
            let (a, b) = (&blocks[left].bounds, &blocks[right].bounds);
            let overlap = a.right.min(b.right) - a.left.max(b.left);
            let narrower = (a.right - a.left).min(b.right - b.left);
            // 重叠超过较窄一方的一半才算同一栏，避免两栏之间偶然擦边就连成一片。
            if overlap > 0.0 && overlap * 2.0 >= narrower {
                let (a, b) = (root(&mut column, left), root(&mut column, right));
                column[a] = b;
            }
        }
    }
    let mut columns = HashMap::<usize, Vec<OcrTextBlock>>::new();
    for (index, block) in blocks.into_iter().enumerate() {
        columns
            .entry(root(&mut column, index))
            .or_default()
            .push(block);
    }
    let chars = |blocks: &[OcrTextBlock]| -> usize {
        blocks
            .iter()
            .map(|block| meaningful_chars(&block.text))
            .sum()
    };
    let left_edge = |blocks: &[OcrTextBlock]| {
        blocks
            .iter()
            .map(|block| block.bounds.left)
            .fold(f32::INFINITY, f32::min)
    };
    let width = |blocks: &[OcrTextBlock]| {
        blocks
            .iter()
            .map(|block| block.bounds.right)
            .fold(f32::NEG_INFINITY, f32::max)
            - left_edge(blocks)
    };
    let Some((main_chars, main_width)) = columns
        .values()
        .max_by_key(|blocks| chars(blocks))
        .map(|blocks| (chars(blocks), width(blocks)))
    else {
        return Vec::new();
    };
    // 字数之外还要看宽度：统计面板、属性栏满是数字和英文，字数可能接近正文，但都比正文窄得多。
    let mut kept = columns
        .into_values()
        .filter(|blocks| chars(blocks) * 2 >= main_chars && width(blocks) * 2.0 >= main_width)
        .collect::<Vec<_>>();
    kept.sort_by(|a, b| left_edge(a).total_cmp(&left_edge(b)));
    kept
}

fn sort_blocks_by_reading_order(blocks: &mut [OcrTextBlock]) {
    blocks.sort_by(|a, b| {
        a.bounds
            .top
            .total_cmp(&b.bounds.top)
            .then_with(|| a.bounds.left.total_cmp(&b.bounds.left))
    });

    let mut line_start = 0;
    while line_start < blocks.len() {
        let line_top = blocks[line_start].bounds.top;
        let mut line_end = line_start + 1;
        while line_end < blocks.len() && blocks[line_end].bounds.top - line_top < 0.015 {
            line_end += 1;
        }
        blocks[line_start..line_end].sort_by(|a, b| {
            a.bounds
                .left
                .total_cmp(&b.bounds.left)
                .then_with(|| a.bounds.top.total_cmp(&b.bounds.top))
        });
        line_start = line_end;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ocr::NormalizedRegion;

    fn block(left: f32, top: f32, text: &str) -> OcrTextBlock {
        wide(left, left + 0.01, top, text)
    }

    fn wide(left: f32, right: f32, top: f32, text: &str) -> OcrTextBlock {
        OcrTextBlock {
            text: text.into(),
            confidence: 1.0,
            bounds: NormalizedRegion {
                left,
                top,
                right,
                bottom: top + 0.02,
            },
        }
    }

    fn texts(blocks: &[OcrTextBlock]) -> Vec<&str> {
        blocks.iter().map(|block| block.text.as_str()).collect()
    }

    /// 截图来自一个左侧会话列表、中间对话、右侧小面板的窗口：只留对话正文，
    /// 并去掉图标误认出的碎片和乱码。
    #[test]
    fn cleanup_keeps_the_main_column_and_drops_fragments() {
        let blocks = finalize_blocks(vec![
            wide(0.02, 0.15, 0.10, "手机运行程序清理"),
            wide(0.02, 0.12, 0.14, "GKD 开屏广告规则"),
            wide(0.02, 0.05, 0.02, "三口"),
            wide(
                0.30,
                0.85,
                0.10,
                "胶囊变慢的原因已经找到：听写把文字粘贴进输入框后，要等剪贴板恢复",
            ),
            wide(
                0.30,
                0.80,
                0.14,
                "现在改成准备粘贴前就让胶囊开始退场，退场动画和文字上屏同时发生",
            ),
            wide(0.30, 0.40, 0.18, "已运行 2 个命令"),
            wide(0.32, 0.36, 0.22, "+新"),
            wide(0.30, 0.34, 0.26, "目>…—·"),
            wide(0.40, 0.42, 0.30, "2026"),
            wide(0.88, 0.98, 0.10, "Free space"),
        ]);
        assert_eq!(
            texts(&blocks),
            vec![
                "胶囊变慢的原因已经找到：听写把文字粘贴进输入框后，要等剪贴板恢复",
                "现在改成准备粘贴前就让胶囊开始退场，退场动画和文字上屏同时发生",
                "已运行 2 个命令",
                "2026",
            ]
        );
    }

    /// 两栏字数相当（如文档旁的对话面板）时都保留，读完左栏再读右栏，同一栏里的句子不被另一栏打断。
    #[test]
    fn comparable_side_by_side_columns_are_read_one_after_another() {
        let blocks = finalize_blocks(vec![
            wide(0.55, 0.95, 0.10, "右侧对话里的一段回复"),
            wide(0.05, 0.45, 0.10, "左侧文档的第一段正文内容"),
            wide(0.55, 0.95, 0.14, "右侧对话的第二行"),
            wide(0.05, 0.45, 0.14, "左侧文档的第二段正文"),
        ]);
        assert_eq!(
            texts(&blocks),
            vec![
                "左侧文档的第一段正文内容",
                "左侧文档的第二段正文",
                "右侧对话里的一段回复",
                "右侧对话的第二行",
            ]
        );
    }

    /// 右侧窄的统计面板满是数字和英文，字数超过正文一半，但比正文窄得多，不保留。
    #[test]
    fn narrow_number_heavy_panel_is_dropped_despite_its_char_count() {
        let blocks = finalize_blocks(vec![
            wide(
                0.25,
                0.75,
                0.10,
                "开发版跟着启动器运行，启动器的终端被关掉后应用随之退出",
            ),
            wide(0.25, 0.70, 0.14, "现在端口空闲，我正在重新启动"),
            wide(0.25, 0.72, 0.18, "这次退出不是代码出了问题"),
            wide(0.80, 0.98, 0.10, "MCP tools 18.6k"),
            wide(0.80, 0.98, 0.14, "System prompt 11.3k"),
            wide(0.80, 0.98, 0.18, "Free space 689.3k"),
        ]);
        assert_eq!(
            texts(&blocks),
            vec![
                "开发版跟着启动器运行，启动器的终端被关掉后应用随之退出",
                "现在端口空闲，我正在重新启动",
                "这次退出不是代码出了问题",
            ]
        );
    }

    #[test]
    fn reading_order_sort_is_total_even_for_overlapping_line_tolerances() {
        let mut blocks = vec![
            block(0.9, 0.0, "a"),
            block(0.5, 0.01, "b"),
            block(0.1, 0.02, "c"),
            block(0.8, 0.005, "d"),
            block(0.4, 0.015, "e"),
            block(0.0, 0.025, "f"),
        ];

        sort_blocks_by_reading_order(&mut blocks);

        assert_eq!(texts(&blocks), vec!["b", "d", "a", "f", "c", "e"]);
    }
}
