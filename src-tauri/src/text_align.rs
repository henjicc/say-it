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

// 仿射 gap 计分（Gotoh）：长插入/删除只收一次开口费。若用线性 gap，说话人大段
// 即兴时对齐会倾向把文稿“就近替换”到无关内容上，而不是跳过插入命中真实匹配。
const SCORE_MATCH: i32 = 8;
const SCORE_MISMATCH: i32 = -4;
const GAP_OPEN: i32 = -6;
const GAP_EXTEND: i32 = -1;
const NEG: i32 = i32::MIN / 4;

/// 三个对齐状态：0=M（对角）、1=Ix（文稿 token 落空）、2=Iy（跳过 ASR token）。
fn best3(m: i32, ix: i32, iy: i32) -> (i32, u8) {
    if m >= ix && m >= iy {
        (m, 0)
    } else if ix >= iy {
        (ix, 1)
    } else {
        (iy, 2)
    }
}

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
    /// 行与其命中音频区间的双向相似度（Dice：2×匹配 / (行 token + 区间 ASR token)）。
    /// 与 match_ratio 的差别：音频区间里说了大量文稿之外的内容时也会显著降低，
    /// 供“差异过大的行改用识别文本”决策使用。
    pub similarity: f32,
    /// 行时间来自相邻行内插而非自身命中。
    pub interpolated: bool,
    /// 行命中（匹配或替换对）覆盖的 ASR 词范围，为输入 words 的下标（含两端）；无命中为 None。
    pub asr_word_begin: Option<usize>,
    pub asr_word_end: Option<usize>,
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
    /// 所属词在输入 words 中的原始下标。
    word_index: usize,
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
    let mut similarities: Vec<f32> = Vec::with_capacity(line_ranges.len());
    let mut word_ranges: Vec<Option<(usize, usize)>> = Vec::with_capacity(line_ranges.len());
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
        match (first_hit, last_hit) {
            (Some(first), Some(last)) => {
                // 双向相似度：命中区间内未匹配的 ASR token（音频里多说的内容）同样拉低相似度
                let span_tokens = last - first + 1;
                similarities.push(2.0 * match_count as f32 / (tokens + span_tokens) as f32);
                word_ranges.push(Some((
                    asr_tokens[first].word_index,
                    asr_tokens[last].word_index,
                )));
            }
            _ => {
                similarities.push(0.0);
                word_ranges.push(None);
            }
        }
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
            similarity: similarities[i],
            interpolated: interpolated[i],
            asr_word_begin: word_ranges[i].map(|(begin, _)| begin),
            asr_word_end: word_ranges[i].map(|(_, end)| end),
        })
        .collect())
}

/// 把 ASR 词按时间排序后拆成 token，多 token 词内部按字符数线性内插时间。
fn build_asr_tokens(words: &[AlignWord]) -> Vec<AsrToken> {
    let mut sorted: Vec<(usize, &AlignWord)> = words.iter().enumerate().collect();
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

/// 全矩阵仿射 gap 对齐（Gotoh 三状态）。free_start / free_end 为 ASR 侧首/尾 gap 免罚
/// （半全局对齐：容忍音频里存在文稿之外的片头/片尾内容）。
/// 回溯字节布局：bit0-1 = M 的来源状态，bit2-3 = Ix 的来源状态，bit4-5 = Iy 的来源状态。
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
    let mut tb = vec![0u8; (n + 1) * width];
    let mut m_prev = vec![NEG; width];
    let mut ix_prev = vec![NEG; width];
    let mut iy_prev = vec![NEG; width];
    let mut m_cur = vec![NEG; width];
    let mut ix_cur = vec![NEG; width];
    let mut iy_cur = vec![NEG; width];

    m_prev[0] = 0;
    for j in 1..=m {
        iy_prev[j] = if free_start { 0 } else { GAP_OPEN + GAP_EXTEND * (j as i32 - 1) };
        if j >= 2 {
            tb[j] = 2 << 4;
        }
    }
    for i in 1..=n {
        m_cur[0] = NEG;
        iy_cur[0] = NEG;
        ix_cur[0] = GAP_OPEN + GAP_EXTEND * (i as i32 - 1);
        tb[i * width] = if i >= 2 { 1 << 2 } else { 0 };
        for j in 1..=m {
            let subst = if s[i - 1] == a[j - 1] { SCORE_MATCH } else { SCORE_MISMATCH };
            let (diag_best, diag_state) = best3(m_prev[j - 1], ix_prev[j - 1], iy_prev[j - 1]);
            m_cur[j] = diag_best + subst;
            let (ix_best, ix_state) = best3(
                m_prev[j] + GAP_OPEN,
                ix_prev[j] + GAP_EXTEND,
                iy_prev[j] + GAP_OPEN,
            );
            ix_cur[j] = ix_best;
            let (iy_best, iy_state) = best3(
                m_cur[j - 1] + GAP_OPEN,
                ix_cur[j - 1] + GAP_OPEN,
                iy_cur[j - 1] + GAP_EXTEND,
            );
            iy_cur[j] = iy_best;
            tb[i * width + j] = diag_state | (ix_state << 2) | (iy_state << 4);
        }
        std::mem::swap(&mut m_prev, &mut m_cur);
        std::mem::swap(&mut ix_prev, &mut ix_cur);
        std::mem::swap(&mut iy_prev, &mut iy_cur);
    }
    // *_prev 此时是最后一行
    let (mut j, mut state) = {
        let (_, st) = best3(m_prev[m], ix_prev[m], iy_prev[m]);
        (m, st)
    };
    if free_end {
        let mut best = NEG;
        for jj in 0..=m {
            let (value, st) = best3(m_prev[jj], ix_prev[jj], iy_prev[jj]);
            if value > best {
                best = value;
                j = jj;
                state = st;
            }
        }
    }
    let mut i = n;
    while i > 0 || j > 0 {
        if i == 0 {
            state = 2;
        } else if j == 0 {
            state = 1;
        }
        let flags = tb[i * width + j];
        match state {
            0 => {
                i -= 1;
                j -= 1;
                links[s_off + i] = if s[i] == a[j] {
                    TokenLink::Match(a_off + j)
                } else {
                    TokenLink::Sub(a_off + j)
                };
                state = flags & 0b11;
            }
            1 => {
                i -= 1;
                state = (flags >> 2) & 0b11;
            }
            _ => {
                j -= 1;
                state = (flags >> 4) & 0b11;
            }
        }
    }
}

/// 带宽限制的仿射 gap 对齐兜底：只计算对角带内的单元格。该路径仅在段超大且完全
/// 找不到锚点（两侧文本高度不相似或高度重复）时触发，带外视为不可达，用质量换内存。
/// 回溯字节布局与 nw_full 相同。
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
    let band_lo = |i: usize| -> usize { (i * m / n).saturating_sub(half) };
    let band_hi = |i: usize| -> usize { (i * m / n + half).min(m) };
    // 行分数只保留带内值，带外读取一律视为不可达
    let read = |row: &[i32], lo: usize, j: usize| -> i32 {
        if j < lo || j >= lo + row.len() {
            NEG
        } else {
            row[j - lo]
        }
    };

    let mut tb = vec![0u8; (n + 1) * bw];
    let mut prev_lo = band_lo(0);
    let prev_hi0 = band_hi(0);
    let mut m_prev: Vec<i32> = vec![NEG; prev_hi0 - prev_lo + 1];
    let mut ix_prev: Vec<i32> = vec![NEG; prev_hi0 - prev_lo + 1];
    let mut iy_prev: Vec<i32> = (prev_lo..=prev_hi0)
        .map(|j| {
            if j == 0 {
                NEG
            } else if free_start {
                0
            } else {
                GAP_OPEN + GAP_EXTEND * (j as i32 - 1)
            }
        })
        .collect();
    m_prev[0] = 0; // band_lo(0) == 0
    for j in prev_lo..=prev_hi0 {
        if j >= 2 {
            tb[j - prev_lo] = 2 << 4;
        }
    }
    for i in 1..=n {
        let lo = band_lo(i);
        let hi = band_hi(i);
        let mut m_cur: Vec<i32> = vec![NEG; hi - lo + 1];
        let mut ix_cur: Vec<i32> = vec![NEG; hi - lo + 1];
        let mut iy_cur: Vec<i32> = vec![NEG; hi - lo + 1];
        for j in lo..=hi {
            if j == 0 {
                ix_cur[0] = GAP_OPEN + GAP_EXTEND * (i as i32 - 1);
                tb[i * bw] = if i >= 2 { 1 << 2 } else { 0 };
                continue;
            }
            let subst = if s[i - 1] == a[j - 1] { SCORE_MATCH } else { SCORE_MISMATCH };
            let (diag_best, diag_state) = best3(
                read(&m_prev, prev_lo, j - 1),
                read(&ix_prev, prev_lo, j - 1),
                read(&iy_prev, prev_lo, j - 1),
            );
            m_cur[j - lo] = diag_best + subst;
            let (ix_best, ix_state) = best3(
                read(&m_prev, prev_lo, j) + GAP_OPEN,
                read(&ix_prev, prev_lo, j) + GAP_EXTEND,
                read(&iy_prev, prev_lo, j) + GAP_OPEN,
            );
            ix_cur[j - lo] = ix_best;
            let (iy_best, iy_state) = best3(
                read(&m_cur, lo, j - 1) + GAP_OPEN,
                read(&ix_cur, lo, j - 1) + GAP_OPEN,
                read(&iy_cur, lo, j - 1) + GAP_EXTEND,
            );
            iy_cur[j - lo] = iy_best;
            tb[i * bw + (j - lo)] = diag_state | (ix_state << 2) | (iy_state << 4);
        }
        m_prev = m_cur;
        ix_prev = ix_cur;
        iy_prev = iy_cur;
        prev_lo = lo;
    }

    let (mut j, mut state) = {
        let (_, st) = best3(
            read(&m_prev, prev_lo, m),
            read(&ix_prev, prev_lo, m),
            read(&iy_prev, prev_lo, m),
        );
        (m, st)
    };
    if free_end {
        let mut best = NEG;
        for off in 0..m_prev.len() {
            let (value, st) = best3(m_prev[off], ix_prev[off], iy_prev[off]);
            if value > best {
                best = value;
                j = prev_lo + off;
                state = st;
            }
        }
    }
    let mut i = n;
    while i > 0 || j > 0 {
        let lo = band_lo(i);
        let hi = band_hi(i);
        if i == 0 {
            state = 2;
        } else if j == 0 {
            state = 1;
        } else if j < lo {
            // 回溯滑出带外时向可行方向收敛，保证终止
            state = 1;
        } else if j > hi {
            state = 2;
        }
        let flags = if j >= lo && j <= hi { tb[i * bw + (j - lo)] } else { 0 };
        match state {
            0 => {
                i -= 1;
                j -= 1;
                links[s_off + i] = if s[i] == a[j] {
                    TokenLink::Match(a_off + j)
                } else {
                    TokenLink::Sub(a_off + j)
                };
                state = flags & 0b11;
            }
            1 => {
                i -= 1;
                state = (flags >> 2) & 0b11;
            }
            _ => {
                j -= 1;
                state = (flags >> 4) & 0b11;
            }
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
        assert!(out.iter().all(|l| (l.similarity - 1.0).abs() < f32::EPSILON));
        assert_eq!((out[0].asr_word_begin, out[0].asr_word_end), (Some(0), Some(2)));
        assert_eq!((out[1].asr_word_begin, out[1].asr_word_end), (Some(3), Some(4)));
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
        assert!(out[1].similarity < 0.4, "整行不匹配的行相似度应显著偏低");
        assert!(out[0].similarity > 0.8 && out[2].similarity > 0.8);
        assert!(out[1].begin_ms >= out[0].end_ms);
        assert!(out[1].end_ms <= out[2].begin_ms);
        assert_timeline_valid(&out);
    }

    #[test]
    fn similarity_drops_when_audio_says_more() {
        // 行内 token 全部匹配，但该行音频区间里说了大量文稿之外的内容：
        // match_ratio 仍为 1，similarity 必须显著下降（双向相似度的意义所在）
        let words = char_words("开场白其实这里即兴发挥了非常非常多的内容结束语", 0, 100);
        let out = align_script(&words, &lines(&["开场白结束语"])).unwrap();
        assert!((out[0].match_ratio - 1.0).abs() < f32::EPSILON);
        assert!(out[0].similarity < 0.6, "similarity={}", out[0].similarity);
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
