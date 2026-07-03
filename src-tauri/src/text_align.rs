//! 文稿对齐纯算法：把 ASR 词级时间戳映射到用户文稿行，产出行级字幕时间轴。
//! 本模块不依赖 tauri，可独立单元测试。
//!
//! 思路（forced alignment 的文本域近似）：
//! 1. 两侧文本规整成 token 序列（CJK 单字、拉丁连串、数字逐字符，中文数字字符等价为阿拉伯数字）；
//! 2. 半全局对齐（ASR 侧首尾 gap 免罚，容忍音频里存在文稿之外的片头/片尾内容）：
//!    小段直接 Needleman-Wunsch，大段先用唯一 n-gram 锚点分治，无锚点时带宽 DP 兜底；
//! 3. 行时间取该行命中 token（匹配或替换对）的首尾时间，命中太少的行按相邻行内插；
//! 4. 后处理保证时间轴单调、不重叠、非空行不短于最小时长。

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// 每条字幕的最小展示时长。
pub const MIN_LINE_DURATION_MS: u64 = 300;
/// 段内直接跑全矩阵 Needleman-Wunsch 的规模上限（单元格数，u8 回溯矩阵约 4MB）。
const FULL_NW_CELL_LIMIT: usize = 4_000_000;
/// 带宽兜底 DP 在长度差之外的带宽余量。
const BAND_MARGIN: usize = 128;
/// 带宽兜底 DP 的内存硬上限（单元格数），超出时收窄带宽，用质量换内存。
const BAND_CELL_LIMIT: usize = 64_000_000;
/// 锚点 n-gram 长度，从大到小尝试。
const ANCHOR_NGRAM_SIZES: [usize; 2] = [5, 3];

const SCORE_MATCH: i32 = 2;
const SCORE_MISMATCH: i32 = -1;
const SCORE_GAP: i32 = -1;

/// 对齐输入的词级时间戳（来自录音识别结果 `sentences[].words[]` 拍平）。
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlignWord {
    #[serde(default)]
    pub begin_time: u64,
    #[serde(default)]
    pub end_time: u64,
    #[serde(default)]
    pub text: String,
}

/// 对齐输出的行级字幕。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlignedLine {
    pub line_index: usize,
    pub text: String,
    pub begin_ms: u64,
    pub end_ms: u64,
    /// 真匹配 token 数 / 行 token 数，供界面提示文稿与音频不符的行。
    pub match_ratio: f32,
    /// 行时间来自相邻行内插而非自身命中。
    pub interpolated: bool,
}

/// 文稿 token 与 ASR token 的对应关系。
#[derive(Clone, Copy, Debug)]
enum TokenLink {
    /// 未对上（gap）。
    None,
    /// 替换对（识别错字）：位置大概率正确，只用于计时，不计入匹配率。
    Sub(usize),
    /// 完全匹配。
    Match(usize),
}

struct AsrToken {
    canon: String,
    begin_ms: u64,
    end_ms: u64,
}

/// 对齐主入口：`script_lines` 一行一句，输出与输入行一一对应。
pub fn align_script(words: &[AlignWord], script_lines: &[String]) -> Result<Vec<AlignedLine>, String> {
    if script_lines.is_empty() {
        return Ok(Vec::new());
    }
    let asr_tokens = build_asr_tokens(words);
    if asr_tokens.is_empty() {
        return Err("识别结果中没有可用的词级时间戳，无法对齐".to_string());
    }

    let mut script_texts: Vec<String> = Vec::new();
    let mut line_ranges: Vec<(usize, usize)> = Vec::with_capacity(script_lines.len());
    for line in script_lines {
        let start = script_texts.len();
        script_texts.extend(tokenize_text(line));
        line_ranges.push((start, script_texts.len()));
    }

    let (script_ids, asr_ids) = intern_ids(&script_texts, &asr_tokens);
    let links = align_tokens(&script_ids, &asr_ids);

    let mut timings: Vec<Option<(u64, u64)>> = Vec::with_capacity(line_ranges.len());
    let mut ratios: Vec<f32> = Vec::with_capacity(line_ranges.len());
    for &(start, end) in &line_ranges {
        let tokens = end - start;
        let mut match_count = 0usize;
        let mut hit_count = 0usize;
        let mut first_hit: Option<usize> = None;
        let mut last_hit: Option<usize> = None;
        for link in &links[start..end] {
            let target = match *link {
                TokenLink::Match(j) => {
                    match_count += 1;
                    Some(j)
                }
                TokenLink::Sub(j) => Some(j),
                TokenLink::None => None,
            };
            if let Some(j) = target {
                hit_count += 1;
                if first_hit.is_none() {
                    first_hit = Some(j);
                }
                last_hit = Some(j);
            }
        }
        ratios.push(if tokens == 0 {
            0.0
        } else {
            match_count as f32 / tokens as f32
        });
        // 命中太少的行不信任自身命中（CJK 常见字在差异区可能随机配对导致边界漂移），改用内插
        let reliable = (hit_count >= 2 && hit_count * 5 >= tokens)
            || (tokens > 0 && tokens <= 3 && match_count >= 1);
        if reliable {
            timings.push(Some((
                asr_tokens[first_hit.expect("可靠行必有命中")].begin_ms,
                asr_tokens[last_hit.expect("可靠行必有命中")].end_ms,
            )));
        } else {
            timings.push(None);
        }
    }

    let interpolated: Vec<bool> = timings.iter().map(Option::is_none).collect();
    let weights: Vec<usize> = line_ranges.iter().map(|&(s, e)| e - s).collect();
    let audio_begin = asr_tokens.first().map(|t| t.begin_ms).unwrap_or(0);
    let audio_end = asr_tokens.last().map(|t| t.end_ms).unwrap_or(0);
    let mut resolved = fill_missing(&timings, &weights, audio_begin, audio_end);
    let non_empty: Vec<bool> = weights.iter().map(|&w| w > 0).collect();
    post_process(&mut resolved, &non_empty);

    Ok(script_lines
        .iter()
        .enumerate()
        .map(|(i, line)| AlignedLine {
            line_index: i,
            text: line.trim().to_string(),
            begin_ms: resolved[i].0,
            end_ms: resolved[i].1,
            match_ratio: ratios[i],
            interpolated: interpolated[i],
        })
        .collect())
}

/// 把 ASR 词按时间排序后拆成 token，多 token 词内部按字符数线性内插时间。
fn build_asr_tokens(words: &[AlignWord]) -> Vec<AsrToken> {
    let mut sorted: Vec<&AlignWord> = words.iter().collect();
    sorted.sort_by_key(|w| w.begin_time);
    let mut tokens = Vec::new();
    for word in sorted {
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
            });
        }
    }
    tokens
}

/// 规整并切分文本：CJK 单字一 token、连续拉丁字母一 token、数字逐字符一 token，
/// 标点/空白/符号只作分隔。
fn tokenize_text(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut latin = String::new();
    for raw in text.chars() {
        let c = canonical_char(raw);
        if c.is_ascii_digit() || is_cjk(c) {
            flush_latin(&mut latin, &mut tokens);
            tokens.push(c.to_string());
        } else if c.is_alphabetic() {
            latin.push(c);
        } else {
            flush_latin(&mut latin, &mut tokens);
        }
    }
    flush_latin(&mut latin, &mut tokens);
    tokens
}

fn flush_latin(latin: &mut String, tokens: &mut Vec<String>) {
    if !latin.is_empty() {
        tokens.push(std::mem::take(latin));
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

/// token 文本内化为整数 id，加速比较与 n-gram 哈希。
fn intern_ids(script: &[String], asr: &[AsrToken]) -> (Vec<u32>, Vec<u32>) {
    let mut map: HashMap<&str, u32> = HashMap::new();
    let mut script_ids = Vec::with_capacity(script.len());
    for t in script {
        let next = map.len() as u32;
        script_ids.push(*map.entry(t.as_str()).or_insert(next));
    }
    let mut asr_ids = Vec::with_capacity(asr.len());
    for t in asr {
        let next = map.len() as u32;
        asr_ids.push(*map.entry(t.canon.as_str()).or_insert(next));
    }
    (script_ids, asr_ids)
}

/// 分治对齐调度：小段直接 NW，大段找锚点切分，无锚点时带宽兜底。
/// 用显式栈代替递归，避免锚点层级过深时栈溢出。
fn align_tokens(script: &[u32], asr: &[u32]) -> Vec<TokenLink> {
    let mut links = vec![TokenLink::None; script.len()];
    if script.is_empty() || asr.is_empty() {
        return links;
    }
    let mut stack = vec![(0usize, script.len(), 0usize, asr.len(), true, true)];
    while let Some((s_lo, s_hi, a_lo, a_hi, free_start, free_end)) = stack.pop() {
        let s = &script[s_lo..s_hi];
        let a = &asr[a_lo..a_hi];
        if s.is_empty() || a.is_empty() {
            continue;
        }
        if s.len().saturating_mul(a.len()) <= FULL_NW_CELL_LIMIT {
            nw_full(s, a, s_lo, a_lo, free_start, free_end, &mut links);
            continue;
        }
        let mut anchored = false;
        for &n in &ANCHOR_NGRAM_SIZES {
            let anchors = find_anchors(s, a, n);
            if anchors.is_empty() {
                continue;
            }
            for &(si, ai) in &anchors {
                for k in 0..n {
                    links[s_lo + si + k] = TokenLink::Match(a_lo + ai + k);
                }
            }
            let mut seg_s = 0;
            let mut seg_a = 0;
            let mut seg_free = free_start;
            for &(si, ai) in &anchors {
                stack.push((s_lo + seg_s, s_lo + si, a_lo + seg_a, a_lo + ai, seg_free, false));
                seg_s = si + n;
                seg_a = ai + n;
                seg_free = false;
            }
            stack.push((s_lo + seg_s, s_hi, a_lo + seg_a, a_hi, false, free_end));
            anchored = true;
            break;
        }
        if !anchored {
            nw_banded(s, a, s_lo, a_lo, free_start, free_end, &mut links);
        }
    }
    links
}

/// 找两侧都只出现一次且相等的 n-gram 作锚点；最长递增子序列保证锚点单调不交叉，
/// 再去掉相互重叠的锚点。
fn find_anchors(s: &[u32], a: &[u32], n: usize) -> Vec<(usize, usize)> {
    if s.len() < n || a.len() < n {
        return Vec::new();
    }
    #[derive(Default)]
    struct Entry {
        s_count: u32,
        s_pos: usize,
        a_count: u32,
        a_pos: usize,
    }
    let mut map: HashMap<&[u32], Entry> = HashMap::new();
    for i in 0..=s.len() - n {
        let e = map.entry(&s[i..i + n]).or_default();
        e.s_count += 1;
        e.s_pos = i;
    }
    for j in 0..=a.len() - n {
        let e = map.entry(&a[j..j + n]).or_default();
        e.a_count += 1;
        e.a_pos = j;
    }
    let mut candidates: Vec<(usize, usize)> = map
        .values()
        .filter(|e| e.s_count == 1 && e.a_count == 1)
        .map(|e| (e.s_pos, e.a_pos))
        .collect();
    candidates.sort_unstable();
    let picked = longest_increasing_by_a(&candidates);
    let mut out: Vec<(usize, usize)> = Vec::with_capacity(picked.len());
    for (si, ai) in picked {
        if let Some(&(ps, pa)) = out.last() {
            if si < ps + n || ai < pa + n {
                continue;
            }
        }
        out.push((si, ai));
    }
    out
}

/// candidates 已按文稿位置升序且互不相同；选出 ASR 位置严格递增的最长子序列。
fn longest_increasing_by_a(candidates: &[(usize, usize)]) -> Vec<(usize, usize)> {
    if candidates.is_empty() {
        return Vec::new();
    }
    let mut tails: Vec<usize> = Vec::new();
    let mut prev: Vec<usize> = vec![usize::MAX; candidates.len()];
    for (idx, &(_, ai)) in candidates.iter().enumerate() {
        let pos = tails.partition_point(|&t| candidates[t].1 < ai);
        if pos > 0 {
            prev[idx] = tails[pos - 1];
        }
        if pos == tails.len() {
            tails.push(idx);
        } else {
            tails[pos] = idx;
        }
    }
    let mut out = Vec::with_capacity(tails.len());
    let mut cur = *tails.last().expect("candidates 非空则 tails 非空");
    loop {
        out.push(candidates[cur]);
        if prev[cur] == usize::MAX {
            break;
        }
        cur = prev[cur];
    }
    out.reverse();
    out
}

/// 全矩阵 Needleman-Wunsch。free_start / free_end 为 ASR 侧首/尾 gap 免罚
/// （半全局对齐：容忍音频里存在文稿之外的片头/片尾内容）。
fn nw_full(
    s: &[u32],
    a: &[u32],
    s_off: usize,
    a_off: usize,
    free_start: bool,
    free_end: bool,
    links: &mut [TokenLink],
) {
    let n = s.len();
    let m = a.len();
    let width = m + 1;
    // 回溯方向：1=对角（匹配/替换）、2=向上（文稿 token 落空）、3=向左（跳过 ASR token）
    let mut tb = vec![0u8; (n + 1) * width];
    let mut prev = vec![0i32; width];
    let mut cur = vec![0i32; width];
    for j in 1..=m {
        prev[j] = if free_start { 0 } else { j as i32 * SCORE_GAP };
        tb[j] = 3;
    }
    for i in 1..=n {
        cur[0] = i as i32 * SCORE_GAP;
        tb[i * width] = 2;
        for j in 1..=m {
            let diag = prev[j - 1]
                + if s[i - 1] == a[j - 1] {
                    SCORE_MATCH
                } else {
                    SCORE_MISMATCH
                };
            let up = prev[j] + SCORE_GAP;
            let left = cur[j - 1] + SCORE_GAP;
            let (best, dir) = if diag >= up && diag >= left {
                (diag, 1)
            } else if up >= left {
                (up, 2)
            } else {
                (left, 3)
            };
            cur[j] = best;
            tb[i * width + j] = dir;
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    // prev 此时是最后一行
    let mut j = m;
    if free_end {
        let mut best = prev[m];
        for (jj, &score) in prev.iter().enumerate() {
            if score > best {
                best = score;
                j = jj;
            }
        }
    }
    let mut i = n;
    while i > 0 || j > 0 {
        let dir = if i == 0 {
            3
        } else if j == 0 {
            2
        } else {
            tb[i * width + j]
        };
        match dir {
            1 => {
                i -= 1;
                j -= 1;
                links[s_off + i] = if s[i] == a[j] {
                    TokenLink::Match(a_off + j)
                } else {
                    TokenLink::Sub(a_off + j)
                };
            }
            2 => i -= 1,
            _ => j -= 1,
        }
    }
}

/// 带宽限制的 NW 兜底：只计算对角带内的单元格。该路径仅在段超大且完全找不到锚点
/// （两侧文本高度不相似或高度重复）时触发，带外视为不可达，用质量换内存。
fn nw_banded(
    s: &[u32],
    a: &[u32],
    s_off: usize,
    a_off: usize,
    free_start: bool,
    free_end: bool,
    links: &mut [TokenLink],
) {
    let n = s.len();
    let m = a.len();
    let mut half = n.abs_diff(m) + BAND_MARGIN;
    let max_width = (BAND_CELL_LIMIT / (n + 1)).max(3);
    if 2 * half + 1 > max_width {
        half = (max_width - 1) / 2;
    }
    let bw = 2 * half + 1;
    let neg = i32::MIN / 4;
    let band_lo = |i: usize| -> usize { (i * m / n).saturating_sub(half) };
    let band_hi = |i: usize| -> usize { (i * m / n + half).min(m) };
    // 行分数只保留带内值，带外读取一律视为不可达
    let read = |row: &[i32], lo: usize, j: usize| -> i32 {
        if j < lo || j >= lo + row.len() {
            neg
        } else {
            row[j - lo]
        }
    };

    let mut tb = vec![0u8; (n + 1) * bw];
    let mut prev_lo = band_lo(0);
    let mut prev: Vec<i32> = (prev_lo..=band_hi(0))
        .map(|j| {
            if j == 0 || free_start {
                0
            } else {
                j as i32 * SCORE_GAP
            }
        })
        .collect();
    for j in prev_lo..=band_hi(0) {
        if j > 0 {
            tb[j - prev_lo] = 3;
        }
    }
    for i in 1..=n {
        let lo = band_lo(i);
        let hi = band_hi(i);
        let mut cur: Vec<i32> = vec![neg; hi - lo + 1];
        for j in lo..=hi {
            let (best, dir) = if j == 0 {
                (i as i32 * SCORE_GAP, 2)
            } else {
                let diag = read(&prev, prev_lo, j - 1)
                    + if s[i - 1] == a[j - 1] {
                        SCORE_MATCH
                    } else {
                        SCORE_MISMATCH
                    };
                let up = read(&prev, prev_lo, j) + SCORE_GAP;
                let left = read(&cur, lo, j - 1) + SCORE_GAP;
                if diag >= up && diag >= left {
                    (diag, 1)
                } else if up >= left {
                    (up, 2)
                } else {
                    (left, 3)
                }
            };
            cur[j - lo] = best;
            tb[i * bw + (j - lo)] = dir;
        }
        prev = cur;
        prev_lo = lo;
    }

    let mut j = m;
    if free_end {
        let mut best = read(&prev, prev_lo, m);
        for (off, &score) in prev.iter().enumerate() {
            if score > best {
                best = score;
                j = prev_lo + off;
            }
        }
    }
    let mut i = n;
    while i > 0 || j > 0 {
        let dir = if i == 0 {
            3
        } else if j == 0 {
            2
        } else {
            let lo = band_lo(i);
            let hi = band_hi(i);
            if j < lo {
                // 回溯滑出带外时向可行方向收敛，保证终止
                2
            } else if j > hi {
                3
            } else {
                tb[i * bw + (j - lo)]
            }
        };
        match dir {
            1 => {
                i -= 1;
                j -= 1;
                links[s_off + i] = if s[i] == a[j] {
                    TokenLink::Match(a_off + j)
                } else {
                    TokenLink::Sub(a_off + j)
                };
            }
            2 => i -= 1,
            _ => j -= 1,
        }
    }
}

/// 无可靠时间的行按相邻已定行的边界内插，按行 token 数加权分摊区间。
fn fill_missing(
    timings: &[Option<(u64, u64)>],
    weights: &[usize],
    audio_begin: u64,
    audio_end: u64,
) -> Vec<(u64, u64)> {
    let n = timings.len();
    let mut out: Vec<(u64, u64)> = vec![(0, 0); n];
    for (i, t) in timings.iter().enumerate() {
        if let Some(v) = t {
            out[i] = *v;
        }
    }
    let mut i = 0;
    while i < n {
        if timings[i].is_some() {
            i += 1;
            continue;
        }
        let start = i;
        let mut end = i;
        while end < n && timings[end].is_none() {
            end += 1;
        }
        let left = if start == 0 { audio_begin } else { out[start - 1].1 };
        let right = (if end == n { audio_end } else { out[end].0 }).max(left);
        let span = right - left;
        let total: u64 = weights[start..end].iter().map(|&w| w as u64).sum();
        let mut acc = 0u64;
        for k in start..end {
            let b = if total == 0 { left } else { left + span * acc / total };
            acc += weights[k] as u64;
            let e = if total == 0 { left } else { left + span * acc / total };
            out[k] = (b, e);
        }
        i = end;
    }
    out
}

/// 保证时间轴单调不重叠，并为非空行提供最小展示时长。
fn post_process(timings: &mut [(u64, u64)], non_empty: &[bool]) {
    for t in timings.iter_mut() {
        if t.1 < t.0 {
            t.1 = t.0;
        }
    }
    for i in 1..timings.len() {
        if timings[i].0 < timings[i - 1].1 {
            // 相邻冲突：在冲突区间中点截断
            let mid = ((timings[i].0 + timings[i - 1].1) / 2).max(timings[i - 1].0);
            timings[i - 1].1 = mid;
            timings[i].0 = mid;
            if timings[i].1 < mid {
                timings[i].1 = mid;
            }
        }
    }
    for i in 0..timings.len() {
        if !non_empty[i] {
            continue;
        }
        let desired = timings[i].0 + MIN_LINE_DURATION_MS;
        if timings[i].1 < desired {
            let cap = if i + 1 < timings.len() {
                timings[i + 1].0
            } else {
                u64::MAX
            };
            timings[i].1 = desired.min(cap).max(timings[i].1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(text: &str, begin: u64, end: u64) -> AlignWord {
        AlignWord {
            begin_time: begin,
            end_time: end,
            text: text.to_string(),
        }
    }

    fn char_words(text: &str, start_ms: u64, step_ms: u64) -> Vec<AlignWord> {
        text.chars()
            .enumerate()
            .map(|(i, c)| {
                let begin = start_ms + i as u64 * step_ms;
                w(&c.to_string(), begin, begin + step_ms)
            })
            .collect()
    }

    fn lines(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    fn assert_timeline_valid(out: &[AlignedLine]) {
        for line in out {
            assert!(line.begin_ms <= line.end_ms, "行 {} 起止倒置", line.line_index);
        }
        for pair in out.windows(2) {
            assert!(
                pair[0].end_ms <= pair[1].begin_ms,
                "行 {} 与行 {} 重叠",
                pair[0].line_index,
                pair[1].line_index
            );
        }
    }

    #[test]
    fn exact_match_uses_word_times() {
        let words = vec![
            w("今天", 0, 600),
            w("天气", 600, 1200),
            w("很好", 1200, 1800),
            w("明天", 2000, 2600),
            w("再见", 2600, 3200),
        ];
        let out = align_script(&words, &lines(&["今天天气很好", "明天再见"])).unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!((out[0].begin_ms, out[0].end_ms), (0, 1800));
        assert_eq!((out[1].begin_ms, out[1].end_ms), (2000, 3200));
        assert!(out
            .iter()
            .all(|l| (l.match_ratio - 1.0).abs() < f32::EPSILON && !l.interpolated));
        assert_timeline_valid(&out);
    }

    #[test]
    fn script_extra_chars_keep_line_times() {
        // 文稿比音频多字（ASR 漏识别），仍按已匹配 token 取行时间
        let words = char_words("今天天气很好", 0, 100);
        let out = align_script(&words, &lines(&["今天天气真的很好"])).unwrap();
        assert_eq!(out[0].begin_ms, 0);
        assert_eq!(out[0].end_ms, 600);
        assert!(out[0].match_ratio < 1.0 && out[0].match_ratio >= 0.7);
        assert!(!out[0].interpolated);
    }

    #[test]
    fn asr_fillers_are_skipped() {
        // ASR 里的语气词/口头语不拉偏行时间
        let words = char_words("嗯今天那个天气很好", 0, 100);
        let out = align_script(&words, &lines(&["今天天气很好"])).unwrap();
        assert_eq!(out[0].begin_ms, 100);
        assert_eq!(out[0].end_ms, 900);
        assert!((out[0].match_ratio - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn substitution_keeps_timing_and_lowers_ratio() {
        // 识别错字（替换对）不影响行时间，但拉低匹配率
        let words = char_words("今天天汽很好", 0, 100);
        let out = align_script(&words, &lines(&["今天天气很好"])).unwrap();
        assert_eq!((out[0].begin_ms, out[0].end_ms), (0, 600));
        assert!(out[0].match_ratio < 1.0);
        assert!(!out[0].interpolated);
    }

    #[test]
    fn unmatched_line_is_interpolated() {
        let mut words = char_words("第一句话说完了", 0, 100);
        words.extend(char_words("第三句话开始了", 2000, 100));
        let out = align_script(
            &words,
            &lines(&["第一句话说完了", "完全无关的内容啊", "第三句话开始了"]),
        )
        .unwrap();
        assert!(out[1].interpolated);
        assert!(out[1].match_ratio < 0.3);
        assert!(out[1].begin_ms >= out[0].end_ms);
        assert!(out[1].end_ms <= out[2].begin_ms);
        assert_timeline_valid(&out);
    }

    #[test]
    fn mixed_cjk_latin() {
        let words = vec![w("我用", 0, 600), w("github", 600, 1200), w("写代码", 1200, 1800)];
        let out = align_script(&words, &lines(&["我用 GitHub 写代码"])).unwrap();
        assert_eq!((out[0].begin_ms, out[0].end_ms), (0, 1800));
        assert!((out[0].match_ratio - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn chinese_digits_match_arabic() {
        let words = char_words("二零二四年发布", 0, 100);
        let out = align_script(&words, &lines(&["2024年发布"])).unwrap();
        assert!((out[0].match_ratio - 1.0).abs() < f32::EPSILON);
        assert_eq!((out[0].begin_ms, out[0].end_ms), (0, 700));
    }

    #[test]
    fn min_duration_is_enforced() {
        let words = vec![w("好", 1000, 1050)];
        let out = align_script(&words, &lines(&["好"])).unwrap();
        assert_eq!(out[0].begin_ms, 1000);
        assert!(out[0].end_ms - out[0].begin_ms >= MIN_LINE_DURATION_MS);
    }

    #[test]
    fn leading_audio_junk_is_free() {
        // 片头与文稿无关的内容不产生罚分，也不拉偏第一行时间（半全局对齐）
        let words = char_words("废话闲聊几句吧正文从这里开始", 0, 100);
        let out = align_script(&words, &lines(&["正文从这里开始"])).unwrap();
        assert_eq!(out[0].begin_ms, 700);
        assert!((out[0].match_ratio - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn empty_inputs() {
        assert!(align_script(&[], &lines(&["你好"])).is_err());
        assert!(align_script(&[w("你好", 0, 100)], &[]).unwrap().is_empty());
    }

    #[test]
    fn blank_line_gets_zero_width_slot() {
        let words = char_words("今天天气很好明天再见", 0, 100);
        let out = align_script(&words, &lines(&["今天天气很好", "", "明天再见"])).unwrap();
        assert!(out[1].interpolated);
        assert_eq!(out[1].begin_ms, out[1].end_ms);
        assert_timeline_valid(&out);
    }

    #[test]
    fn large_input_uses_anchors() {
        // 超过全矩阵 NW 规模上限，走锚点分治路径
        let text: String = (0..2100u32)
            .map(|i| char::from_u32(0x4E00 + i).unwrap())
            .collect();
        let words = char_words(&text, 0, 50);
        let script: Vec<String> = text
            .chars()
            .collect::<Vec<_>>()
            .chunks(50)
            .map(|c| c.iter().collect())
            .collect();
        let out = align_script(&words, &script).unwrap();
        assert_eq!(out.len(), 42);
        assert!(out
            .iter()
            .all(|l| (l.match_ratio - 1.0).abs() < f32::EPSILON && !l.interpolated));
        assert_eq!(out[0].begin_ms, 0);
        assert_eq!(out.last().unwrap().end_ms, 2100 * 50);
        assert_timeline_valid(&out);
    }

    #[test]
    fn unrelated_large_input_falls_back_to_band() {
        // 两侧完全无关且找不到锚点时走带宽兜底：不 panic、匹配率为 0、时间轴仍合法
        let script_text: String = (0..2100u32)
            .map(|i| char::from_u32(0x4E00 + i).unwrap())
            .collect();
        let asr_text: String = (0..2100u32)
            .map(|i| char::from_u32(0x8000 + i).unwrap())
            .collect();
        let words = char_words(&asr_text, 0, 50);
        let script: Vec<String> = script_text
            .chars()
            .collect::<Vec<_>>()
            .chunks(50)
            .map(|c| c.iter().collect())
            .collect();
        let out = align_script(&words, &script).unwrap();
        assert!(out.iter().all(|l| l.match_ratio == 0.0));
        assert_timeline_valid(&out);
    }
}
