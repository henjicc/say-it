//! 动态上下文：听写过程中跟着用户的操作补充上下文——切换窗口、窗口内容变化（滚动、打开图片）、
//! 选中文字、鼠标停留或绕圈指着某处。这里是与平台无关的判定和排版，采集在 `dynamic_monitor.rs`。
#![cfg_attr(not(windows), allow(dead_code))]
use std::collections::{HashSet, VecDeque};

use serde::Serialize;

use super::model::OcrTextBlock;
use super::normalize::truncate_chars;

/// 说话过程中发生的一件事，`at_ms` 从开始录音算起。
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DynamicEntry {
    pub(crate) at_ms: u64,
    pub(crate) kind: DynamicKind,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub(crate) enum DynamicKind {
    /// 切到了另一个窗口，或同一窗口的内容明显变了。`lines` 是这一刻窗口里的文字。
    Window {
        switched: bool,
        app_name: String,
        window_title: Option<String>,
        lines: Vec<String>,
    },
    /// 用户手动选中的文字。
    Selection { text: String },
    /// 鼠标停留或绕圈处的文字。
    Pointer { text: String },
}

/// 整段动态上下文的字数上限。选中和指着的文字排在前面，超出时先丢最早的窗口文字。
pub(crate) const DYNAMIC_MAX_CHARS: usize = 1_500;
/// 每个窗口快照最多保留的新文字。
const WINDOW_ENTRY_CHARS: usize = 500;
/// 选中、指着的文字各自的上限。
const SNIPPET_CHARS: usize = 200;
const HEADER: &str = "说话时的操作（秒数从开始说话算起；说“这个”“这里”时多半指同一时刻选中或鼠标指着的内容）：";

/// 排成提示词里的一段。`seen` 是基础上下文里已有的文字（选中文字和窗口文字行），重复的不再写。
pub(crate) fn format_entries(entries: &[DynamicEntry], seen: &[String]) -> String {
    let mut seen: HashSet<String> = seen.iter().map(|line| line.trim().to_string()).collect();
    let mut lines: Vec<(bool, String)> = Vec::new();
    // 每处指向：（各次时间，最完整的那次文字，在 lines 里的位置）。
    let mut pointers: Vec<(Vec<u64>, String, usize)> = Vec::new();
    for entry in entries {
        let at = format!("[{:.1}秒]", entry.at_ms as f64 / 1000.0);
        let line = match &entry.kind {
            DynamicKind::Window { switched, app_name, window_title, lines: text } => {
                let fresh: Vec<&str> = text
                    .iter()
                    .map(|line| line.trim())
                    .filter(|line| !line.is_empty() && seen.insert(line.to_string()))
                    .collect();
                let title = window_title
                    .as_deref()
                    .filter(|title| !title.is_empty())
                    .map(|title| format!("「{title}」"))
                    .unwrap_or_default();
                let head = match switched {
                    true => format!("{at} 切换到 {app_name}{title}"),
                    false if fresh.is_empty() => continue,
                    false => format!("{at} {app_name}{title} 的内容变了"),
                };
                match fresh.is_empty() {
                    true => (true, head),
                    false => (
                        true,
                        format!("{head}，可见文字：{}", truncate_chars(&fresh.join(" / "), WINDOW_ENTRY_CHARS).0),
                    ),
                }
            }
            // 开始说话前就选中的那段已经在上下文里了，松开 Shift 等又读到它时不再写。
            DynamicKind::Selection { text } if !seen.insert(text.trim().to_string()) => continue,
            DynamicKind::Selection { text } => {
                (false, format!("{at} 选中：{}", truncate_chars(text.trim(), SNIPPET_CHARS).0))
            }
            // 同一处停住几次，每次截到的字常差一两个：几乎相同的合成一行，列出每次的时间。
            DynamicKind::Pointer { text } => {
                let text = text.trim();
                match pointers.iter_mut().find(|(_, known, _)| similar(known, text)) {
                    Some((times, known, _)) => {
                        times.push(entry.at_ms);
                        if meaningful_chars(text) > meaningful_chars(known) {
                            *known = text.to_string();
                        }
                    }
                    None => pointers.push((vec![entry.at_ms], text.to_string(), lines.len())),
                }
                (false, String::new())
            }
        };
        lines.push(line);
    }
    for (times, text, index) in pointers {
        let times = times.iter().map(|at| format!("{:.1}秒", *at as f64 / 1000.0)).collect::<Vec<_>>();
        lines[index].1 = format!("[{}] 鼠标指着：{}", times.join("、"), truncate_chars(&text, SNIPPET_CHARS).0);
    }
    lines.retain(|(_, line)| !line.is_empty());
    if lines.is_empty() {
        return String::new();
    }
    let total = |lines: &[(bool, String)]| {
        HEADER.chars().count() + lines.iter().map(|(_, line)| line.chars().count() + 1).sum::<usize>()
    };
    while total(&lines) > DYNAMIC_MAX_CHARS {
        match lines.iter().position(|(window, _)| *window) {
            Some(index) => {
                lines.remove(index);
            }
            None => break,
        }
    }
    let body = lines.into_iter().map(|(_, line)| line).collect::<Vec<_>>().join("\n");
    truncate_chars(&format!("{HEADER}\n{body}"), DYNAMIC_MAX_CHARS).0
}

fn meaningful_chars(text: &str) -> usize {
    text.chars().filter(|c| c.is_alphanumeric()).count()
}

/// 两段指向文字几乎相同：去掉空白和标点后一段包含另一段，或相邻两字的组合有六成以上相同。
fn similar(a: &str, b: &str) -> bool {
    let normalize = |text: &str| -> Vec<char> {
        text.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
    };
    let (a, b) = (normalize(a), normalize(b));
    if a.is_empty() || b.is_empty() {
        return a == b;
    }
    let (shorter, longer) = if a.len() <= b.len() { (&a, &b) } else { (&b, &a) };
    if longer.windows(shorter.len()).any(|window| window == shorter.as_slice()) {
        return true;
    }
    let pairs = |chars: &[char]| chars.windows(2).map(|pair| (pair[0], pair[1])).collect::<Vec<_>>();
    let (left, mut right) = (pairs(&a), pairs(&b));
    if left.is_empty() || right.is_empty() {
        return false;
    }
    let total = left.len() + right.len();
    let shared = left
        .iter()
        .filter(|pair| match right.iter().position(|other| other == *pair) {
            Some(index) => {
                right.swap_remove(index);
                true
            }
            None => false,
        })
        .count();
    shared * 2 * 10 >= total * 6
}

/// 光标停在半径这么多像素内才算停留。
const DWELL_RADIUS: i32 = 24;
/// 停留这么久算指着。
const DWELL_MS: u64 = 450;
/// 离上一次指着的位置（或开始说话时的位置）这么远才算新的指向：一直放着不动的鼠标不算。
const MOVE_MIN: i32 = 48;
/// 判断绕圈看最近这段时间的轨迹。
const CIRCLE_WINDOW_MS: u64 = 1_500;
/// 轨迹总长至少是外框对角线的这么多倍、外框又不大时，算绕着某处画圈。
const CIRCLE_PATH_RATIO: f64 = 3.0;
const CIRCLE_MAX_DIAGONAL: f64 = 240.0;
const CIRCLE_MIN_PATH: f64 = 300.0;

/// 从光标轨迹里认出「指着某处」：移到新位置后停住，或绕着一处画圈。
pub(crate) struct PointerTracker {
    anchor: (i32, i32),
    dwell: Option<(u64, i32, i32)>,
    dwell_reported: bool,
    trail: VecDeque<(u64, i32, i32)>,
}

impl PointerTracker {
    pub(crate) fn new(start: (i32, i32)) -> Self {
        Self { anchor: start, dwell: None, dwell_reported: false, trail: VecDeque::new() }
    }

    /// 喂入一个采样点，认出指向时返回指着的屏幕坐标。
    pub(crate) fn push(&mut self, at_ms: u64, x: i32, y: i32) -> Option<(i32, i32)> {
        self.trail.push_back((at_ms, x, y));
        while self.trail.front().is_some_and(|(t, _, _)| at_ms - t > CIRCLE_WINDOW_MS) {
            self.trail.pop_front();
        }
        if let Some(center) = self.circle() {
            self.trail.clear();
            self.anchor = center;
            self.dwell = None;
            self.dwell_reported = true;
            return Some(center);
        }
        match self.dwell {
            Some((_, dx, dy)) if distance((dx, dy), (x, y)) <= DWELL_RADIUS as f64 => {}
            _ => {
                self.dwell = Some((at_ms, x, y));
                self.dwell_reported = false;
            }
        }
        let (start, dx, dy) = self.dwell?;
        if self.dwell_reported
            || at_ms - start < DWELL_MS
            || distance((dx, dy), self.anchor) <= MOVE_MIN as f64
        {
            return None;
        }
        self.dwell_reported = true;
        self.anchor = (dx, dy);
        Some((dx, dy))
    }

    fn circle(&self) -> Option<(i32, i32)> {
        let (first, _, _) = *self.trail.front()?;
        let (last, _, _) = *self.trail.back()?;
        if last - first < CIRCLE_WINDOW_MS / 2 {
            return None;
        }
        let path: f64 = self
            .trail
            .iter()
            .zip(self.trail.iter().skip(1))
            .map(|(a, b)| distance((a.1, a.2), (b.1, b.2)))
            .sum();
        let (min_x, max_x) = self.trail.iter().fold((i32::MAX, i32::MIN), |(lo, hi), p| (lo.min(p.1), hi.max(p.1)));
        let (min_y, max_y) = self.trail.iter().fold((i32::MAX, i32::MIN), |(lo, hi), p| (lo.min(p.2), hi.max(p.2)));
        let diagonal = distance((min_x, min_y), (max_x, max_y));
        let circling = path >= CIRCLE_MIN_PATH
            && diagonal >= DWELL_RADIUS as f64
            && diagonal <= CIRCLE_MAX_DIAGONAL
            && path >= diagonal * CIRCLE_PATH_RATIO;
        circling.then_some(((min_x + max_x) / 2, (min_y + max_y) / 2))
    }
}

fn distance(a: (i32, i32), b: (i32, i32)) -> f64 {
    (((a.0 - b.0) as f64).powi(2) + ((a.1 - b.1) as f64).powi(2)).sqrt()
}

/// 鼠标指向时截取光标周围这么大的区域（宽、高，像素），超出光标所在窗口的部分不截。
/// 截得宽一些，光标所在的那一小段才不容易被截断。
pub(crate) const POINTER_REGION: (i32, i32) = (720, 100);
/// 离光标超过这么多像素的文字不算指着的。
const POINTER_REACH: f32 = 40.0;
/// 指文字时光标多停在字的下方一点，免得挡住字：按光标往上挪这么多像素来选行。
const POINTER_LIFT: f32 = 10.0;
/// 光标所在那一小段两边没有标点时，最多取光标左右各这么多字。
const PHRASE_HALF_CHARS: usize = 10;
const WORD_EXTEND_CHARS: usize = 20;
/// 一小段的分界：标点。英文单词之间的空格不算，「segment-map 免费」要整段取。
const PHRASE_BREAKS: &str = "，,。；;：:、！!？?（）()【】[]「」“”\"|";

/// 从光标周围那块区域识别出的文字里，取光标所在的那一行，再取行里光标所在的那一小段
/// （两边到标点为止）。`at` 是光标在截图里的相对位置，`size` 是截图的像素尺寸。
pub(crate) fn pointed_block(blocks: &[OcrTextBlock], at: (f32, f32), size: (f32, f32)) -> Option<String> {
    let lifted = at.1 - POINTER_LIFT / size.1;
    let gap = |block: &OcrTextBlock| {
        let b = &block.bounds;
        let dx = (b.left - at.0).max(at.0 - b.right).max(0.0) * size.0;
        let dy = (b.top - lifted).max(lifted - b.bottom).max(0.0) * size.1;
        (dx * dx + dy * dy).sqrt()
    };
    let (_, block) = blocks
        .iter()
        .map(|block| (gap(block), block))
        .filter(|(gap, block)| *gap <= POINTER_REACH && !block.text.trim().is_empty())
        .min_by(|a, b| a.0.total_cmp(&b.0))?;
    let width = block.bounds.right - block.bounds.left;
    let ratio = if width > 0.0 { ((at.0 - block.bounds.left) / width).clamp(0.0, 1.0) } else { 0.5 };
    phrase_at(block.text.trim(), ratio)
}

/// 行里横向位置 `ratio`（0～1）处的那一小段。按字宽估计位置：汉字等全角字按两个半角宽算。
pub(crate) fn phrase_at(line: &str, ratio: f32) -> Option<String> {
    let widths: Vec<f32> = line.chars().map(|c| if c.is_ascii() { 1.0 } else { 2.0 }).collect();
    let target = widths.iter().sum::<f32>() * ratio;
    let mut covered = 0.0;
    let center = widths
        .iter()
        .position(|width| {
            covered += width;
            covered > target
        })
        .unwrap_or(widths.len().saturating_sub(1));
    phrase_around(line, center)
}

/// 行里第 `center` 个字所在的那一小段：两边到标点为止，没有标点时左右各取约 10 个字。
pub(crate) fn phrase_around(line: &str, center: usize) -> Option<String> {
    let chars: Vec<char> = line.chars().collect();
    let center = center.min(chars.len().checked_sub(1)?);
    let is_break = |index: usize| PHRASE_BREAKS.contains(chars[index]);
    // 光标正好落在标点上时，取它左边那一段。
    let center = (0..=center).rev().find(|&index| !is_break(index))?;
    let start = (0..center)
        .rev()
        .take_while(|&index| !is_break(index))
        .take(PHRASE_HALF_CHARS)
        .last()
        .unwrap_or(center);
    let end = (center + 1..chars.len())
        .take_while(|&index| !is_break(index))
        .take(PHRASE_HALF_CHARS)
        .last()
        .unwrap_or(center);
    // 英文按字数截会断在单词中间，两头各补到单词边界（最多再补这么多个字母）。
    let in_word = |index: usize| chars[index].is_ascii_alphanumeric() || matches!(chars[index], '-' | '_' | '.' | '$');
    let start = (0..start)
        .rev()
        .take(WORD_EXTEND_CHARS)
        .take_while(|&index| in_word(index) && in_word(index + 1))
        .last()
        .unwrap_or(start);
    let end = (end + 1..chars.len())
        .take(WORD_EXTEND_CHARS)
        .take_while(|&index| in_word(index) && in_word(index - 1))
        .last()
        .unwrap_or(end);
    let phrase: String = chars[start..=end].iter().collect::<String>().trim().to_string();
    (!phrase.is_empty()).then_some(phrase)
}

/// 缩略图（灰度）里明显变了的像素占比超过这个值才算窗口内容变了：输入框里多几个字、
/// 光标闪烁、听写胶囊的波形都不该算。
const CHANGED_FRACTION: f64 = 0.06;
const PIXEL_DELTA: u8 = 24;

pub(crate) fn thumbnail_changed(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() || a.is_empty() {
        return true;
    }
    let changed = a.iter().zip(b).filter(|(x, y)| x.abs_diff(**y) > PIXEL_DELTA).count();
    changed as f64 / a.len() as f64 > CHANGED_FRACTION
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(at_ms: u64, switched: bool, lines: &[&str]) -> DynamicEntry {
        DynamicEntry {
            at_ms,
            kind: DynamicKind::Window {
                switched,
                app_name: "Chrome".into(),
                window_title: Some("文档".into()),
                lines: lines.iter().map(|line| line.to_string()).collect(),
            },
        }
    }

    #[test]
    fn entries_skip_text_already_on_screen_and_keep_selection_and_pointer() {
        let text = format_entries(
            &[
                window(1200, true, &["标题", "新段落"]),
                window(2500, false, &["新段落"]),
                DynamicEntry { at_ms: 3000, kind: DynamicKind::Selection { text: " 选中的字 ".into() } },
                DynamicEntry { at_ms: 4100, kind: DynamicKind::Pointer { text: "导出设置".into() } },
            ],
            &["标题".into()],
        );
        assert_eq!(
            text,
            format!(
                "{HEADER}\n[1.2秒] 切换到 Chrome「文档」，可见文字：新段落\n[3.0秒] 选中：选中的字\n[4.1秒] 鼠标指着：导出设置"
            )
        );
        assert_eq!(format_entries(&[], &[]), "");
    }

    #[test]
    fn oldest_window_text_is_dropped_first_when_over_budget() {
        let long = "字".repeat(WINDOW_ENTRY_CHARS);
        let mut entries: Vec<DynamicEntry> = (0..5)
            .map(|index| window(index * 1000, true, &[&format!("{index}{long}")]))
            .collect();
        entries.push(DynamicEntry { at_ms: 9000, kind: DynamicKind::Selection { text: "保留".into() } });
        let text = format_entries(&entries, &[]);
        assert!(text.chars().count() <= DYNAMIC_MAX_CHARS);
        assert!(!text.contains("[0.0秒]"));
        assert!(text.contains("[4.0秒]"));
        assert!(text.ends_with("[9.0秒] 选中：保留"));
    }

    #[test]
    fn resting_cursor_is_not_pointing_but_moving_then_stopping_is() {
        let mut tracker = PointerTracker::new((100, 100));
        assert!((0..30).all(|step| tracker.push(step * 33, 100, 100).is_none()));
        assert!(tracker.push(1_000, 400, 300).is_none());
        let pointed = (0..20).find_map(|step| tracker.push(1_033 + step * 33, 402, 301));
        assert_eq!(pointed, Some((400, 300)));
        // 同一处继续停着不再重复报告。
        assert!((0..20).all(|step| tracker.push(1_700 + step * 33, 401, 300).is_none()));
    }

    #[test]
    fn circling_around_a_spot_is_pointing() {
        let mut tracker = PointerTracker::new((0, 0));
        let pointed = (0..60).find_map(|step| {
            let angle = step as f64 * 0.5;
            tracker.push(step * 33, 500 + (60.0 * angle.cos()) as i32, 300 + (40.0 * angle.sin()) as i32)
        });
        let (x, y) = pointed.expect("绕圈应当被认出");
        assert!((x - 500).abs() <= 10 && (y - 300).abs() <= 10);
    }

    #[test]
    fn pointer_picks_the_text_under_or_nearest_the_cursor() {
        let block = |text: &str, left: f32, top: f32, right: f32, bottom: f32| OcrTextBlock {
            text: text.into(),
            confidence: 0.9,
            bounds: super::super::model::NormalizedRegion { left, top, right, bottom },
        };
        let blocks = [
            block("左边的按钮", 0.0, 0.4, 0.2, 0.6),
            block("导出设置", 0.45, 0.4, 0.7, 0.6),
        ];
        let size = (POINTER_REGION.0 as f32, POINTER_REGION.1 as f32);
        assert_eq!(pointed_block(&blocks, (0.5, 0.5), size).as_deref(), Some("导出设置"));
        // 截图被窗口边缘裁过时光标不在正中。
        assert_eq!(pointed_block(&blocks, (0.1, 0.5), size).as_deref(), Some("左边的按钮"));
        // 光标落在两段之间时取近的；都太远时不算指着。
        assert_eq!(pointed_block(&[block("近", 0.52, 0.4, 0.6, 0.6), block("远", 0.0, 0.0, 0.1, 0.1)], (0.5, 0.5), size).as_deref(), Some("近"));
        assert_eq!(pointed_block(&[block("远", 0.0, 0.0, 0.1, 0.1)], (0.5, 0.5), size), None);
    }

    #[test]
    fn pointer_takes_the_phrase_under_the_cursor_in_the_line_above() {
        let block = |text: &str, top: f32, bottom: f32| OcrTextBlock {
            text: text.into(),
            confidence: 0.9,
            bounds: super::super::model::NormalizedRegion { left: 0.0, top, right: 1.0, bottom },
        };
        let lines = [
            block("核实:segment-map 免费，segment-edit 每张", 0.2, 0.45),
            block("型，并把区域名与平台任务号存入续接引用。", 0.55, 0.8),
        ];
        let size = (POINTER_REGION.0 as f32, POINTER_REGION.1 as f32);
        // 光标停在两行之间、略靠下一行：人指字时光标常在字下方，取上一行。
        assert_eq!(pointed_block(&lines, (0.3, 0.52), size).as_deref(), Some("segment-map 免费"));
        assert_eq!(pointed_block(&lines, (0.75, 0.52), size).as_deref(), Some("segment-edit 每张"));
        // 两边没有标点时只取光标左右各一段。
        assert_eq!(phrase_at(&"字".repeat(40), 0.5).map(|text| text.chars().count()), Some(PHRASE_HALF_CHARS * 2 + 1));
        // 光标落在标点上取它左边那段。
        assert_eq!(phrase_at("甲乙，丙丁", 0.5).as_deref(), Some("甲乙"));
        // 按坐标取到整行时直接给出光标是第几个字。
        let line = "核实:segment-map 免费，segment-edit 每张";
        let at = line.chars().position(|c| c == '每').unwrap();
        assert_eq!(phrase_around(line, at).as_deref(), Some("segment-edit 每张"));
        assert_eq!(phrase_around("", 0), None);
        // 英文不断在单词中间。
        let phrase = phrase_at("The quick brown fox jumps over the lazy dog near the riverbank today", 0.5).unwrap();
        let words: Vec<&str> = "The quick brown fox jumps over the lazy dog near the riverbank today".split(' ').collect();
        assert!(phrase.split(' ').all(|word| words.contains(&word)), "{phrase}");
    }

    #[test]
    fn nearly_identical_pointings_merge_into_one_line_with_every_time() {
        let pointer = |at_ms: u64, text: &str| DynamicEntry { at_ms, kind: DynamicKind::Pointer { text: text.into() } };
        let text = format_entries(
            &[
                pointer(3_600, "费，segment-edit 每张 $0.02；我接下来写这"),
                pointer(5_800, "segment-map 免费"),
                pointer(7_000, "费，segment-edit 每张$0.02；我接下来写这两"),
            ],
            &[],
        );
        assert_eq!(
            text,
            format!("{HEADER}\n[3.6秒、7.0秒] 鼠标指着：费，segment-edit 每张$0.02；我接下来写这两\n[5.8秒] 鼠标指着：segment-map 免费")
        );
        assert!(!similar("segment-map 免费", "segment-edit 每张"));
    }

    #[test]
    fn small_changes_do_not_count_as_new_content() {
        let base = vec![100u8; 1000];
        let mut caret = base.clone();
        caret[..20].fill(255);
        assert!(!thumbnail_changed(&base, &caret));
        let mut scrolled = base.clone();
        scrolled[..200].fill(0);
        assert!(thumbnail_changed(&base, &scrolled));
        assert!(thumbnail_changed(&base, &base[..10]));
    }
}
