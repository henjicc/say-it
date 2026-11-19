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
            DynamicKind::Pointer { text } => {
                (false, format!("{at} 鼠标指着：{}", truncate_chars(text.trim(), SNIPPET_CHARS).0))
            }
        };
        lines.push(line);
    }
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
pub(crate) const POINTER_REGION: (i32, i32) = (480, 120);
/// 离光标超过这么多像素的文字不算指着的。
const POINTER_REACH: f32 = 40.0;

/// 从光标周围那块区域识别出的文字里，取光标所在或离它最近的一段。`at` 是光标在截图里的
/// 相对位置，`size` 是截图的像素尺寸。
pub(crate) fn pointed_block(blocks: &[OcrTextBlock], at: (f32, f32), size: (f32, f32)) -> Option<String> {
    let gap = |block: &OcrTextBlock| {
        let b = &block.bounds;
        let dx = (b.left - at.0).max(at.0 - b.right).max(0.0) * size.0;
        let dy = (b.top - at.1).max(at.1 - b.bottom).max(0.0) * size.1;
        (dx * dx + dy * dy).sqrt()
    };
    blocks
        .iter()
        .map(|block| (gap(block), block))
        .filter(|(gap, block)| *gap <= POINTER_REACH && !block.text.trim().is_empty())
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, block)| block.text.trim().to_string())
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
