use std::collections::HashMap;

use super::tokenize::AsrToken;

/// 段内直接跑全矩阵对齐的规模上限（单元格数，回溯矩阵每格 1 字节）。
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

/// 文稿 token 与 ASR token 的对应关系。
#[derive(Clone, Copy, Debug)]
pub(super) enum TokenLink {
    /// 未对上（gap）。
    None,
    /// 替换对（识别错字）：位置大概率正确，只用于计时，不计入匹配率。
    Sub(usize),
    /// 完全匹配。
    Match(usize),
}

/// token 文本内化为整数 id，加速比较与 n-gram 哈希。
pub(super) fn intern_ids(script: &[String], asr: &[AsrToken]) -> (Vec<u32>, Vec<u32>) {
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
pub(super) fn align_tokens(script: &[u32], asr: &[u32]) -> Vec<TokenLink> {
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
