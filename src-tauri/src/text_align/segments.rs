use super::align::TokenLink;
use super::tokenize::AsrToken;
use super::{AlignedLine, OptimizedSegment, MIN_LINE_DURATION_MS};

/// “识别修正”结果里，连续多少个非完全匹配的文稿 token 才认定为内容确实不同
/// （而非孤立的 ASR 误听），触发丢弃该片段并改用识别文本。取 4：正常 ASR 错字
/// 多为孤立 1~2 字误听，3 字以内更可能是罕见词/专有名词的连续误听（此时文稿更可信，
/// 不应被替换）；只有 4 字以上的连续偏差才足够说明这段内容本身就不同。
const MIN_BAD_RUN_TO_REPLACE: usize = 4;
/// 未被任何文稿片段认领的音频片段，时长达到该阈值才作为“识别插入”片段输出，
/// 过滤掉对齐过程中天然存在的极短间隙（呼吸、极短语气词误差等噪声）。
const MIN_ASR_INSERTION_MS: u64 = 500;

/// 计算“完全按文稿”结果：每行取自身命中 token 的时间，命中不足则相邻内插。
pub(super) fn build_aligned_lines(
    links: &[TokenLink],
    line_ranges: &[(usize, usize)],
    script_lines: &[String],
    asr_tokens: &[AsrToken],
) -> Vec<AlignedLine> {
    let mut timings: Vec<Option<(u64, u64)>> = Vec::with_capacity(line_ranges.len());
    let mut ratios: Vec<f32> = Vec::with_capacity(line_ranges.len());
    for &(start, end) in line_ranges {
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
        ratios.push(if tokens == 0 { 0.0 } else { match_count as f32 / tokens as f32 });
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

    script_lines
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
        .collect()
}

/// 计算“识别修正”结果：见模块头注释思路第 4 点。
pub(super) fn build_optimized_segments(
    links: &[TokenLink],
    line_ranges: &[(usize, usize)],
    token_spans: &[(usize, usize)],
    script_lines: &[String],
    asr_tokens: &[AsrToken],
) -> Vec<OptimizedSegment> {
    let total = links.len();
    let mut claimed = vec![false; asr_tokens.len()];
    let mut tagged: Vec<(u64, OptimizedSegment)> = Vec::new();

    if total > 0 {
        let mut bad: Vec<bool> = links.iter().map(|l| !matches!(l, TokenLink::Match(_))).collect();
        reclassify_short_bad_runs(&mut bad);
        reclassify_hitless_good_runs(&mut bad, links);

        let mut idx = 0;
        while idx < total {
            if bad[idx] {
                // 坏段直接跳过，不产出文稿文本；其覆盖的音频词稍后由“未被认领”扫描统一补齐
                let mut end = idx;
                while end < total && bad[end] {
                    end += 1;
                }
                idx = end;
                continue;
            }
            let line_idx = line_ranges.partition_point(|&(_, e)| e <= idx);
            let line_range = line_ranges[line_idx];
            let mut end = idx;
            while end < total && !bad[end] && end < line_range.1 {
                end += 1;
            }
            let (segment, begin_ms) = build_keep_segment(
                links,
                token_spans,
                script_lines,
                line_idx,
                line_range,
                idx,
                end,
                asr_tokens,
                &mut claimed,
            );
            tagged.push((begin_ms, segment));
            idx = end;
        }
    }

    tagged.extend(collect_orphan_segments(asr_tokens, &claimed));
    tagged.sort_by_key(|(key, _)| *key);
    tagged.into_iter().map(|(_, seg)| seg).collect()
}

/// 把长度小于阈值的“坏”token 连续段重新判为“好”（保留文稿）：孤立的短偏差
/// 大概率是 ASR 噪声或罕见词误听，不构成需要替换的内容差异。
fn reclassify_short_bad_runs(bad: &mut [bool]) {
    let n = bad.len();
    let mut i = 0;
    while i < n {
        if !bad[i] {
            i += 1;
            continue;
        }
        let mut j = i;
        while j < n && bad[j] {
            j += 1;
        }
        if (j - i) < MIN_BAD_RUN_TO_REPLACE {
            for k in i..j {
                bad[k] = false;
            }
        }
        i = j;
    }
}

/// 把命中数为零的“好”token 连续段重新判为“坏”：既然拿不到任何时间信息，
/// 与其保留一段无法定位时间的文稿，不如并入相邻的丢弃区间统一处理。
fn reclassify_hitless_good_runs(bad: &mut [bool], links: &[TokenLink]) {
    let n = bad.len();
    let mut i = 0;
    while i < n {
        if bad[i] {
            i += 1;
            continue;
        }
        let mut j = i;
        let mut has_hit = false;
        while j < n && !bad[j] {
            if !matches!(links[j], TokenLink::None) {
                has_hit = true;
            }
            j += 1;
        }
        if !has_hit {
            for k in i..j {
                bad[k] = true;
            }
        }
        i = j;
    }
}

fn hit_asr_index(link: &TokenLink) -> Option<usize> {
    match link {
        TokenLink::Match(j) | TokenLink::Sub(j) => Some(*j),
        TokenLink::None => None,
    }
}

/// 为一段“好”token 区间 [start,end) 构建保留片段：文本直接切原始文稿子串
/// （首段延伸到行首、末段延伸到行尾，衔接处的标点随前一段保留），时间取区间内
/// 命中 token 的首尾；同时把这些命中标记为“已认领”，供孤儿音频扫描排除。
fn build_keep_segment(
    links: &[TokenLink],
    token_spans: &[(usize, usize)],
    script_lines: &[String],
    line_index: usize,
    line_range: (usize, usize),
    start: usize,
    end: usize,
    asr_tokens: &[AsrToken],
    claimed: &mut [bool],
) -> (OptimizedSegment, u64) {
    let (line_start, line_end) = line_range;
    let text_start = if start == line_start { 0 } else { token_spans[start].0 };
    let text_end = if end == line_end {
        script_lines[line_index].len()
    } else {
        token_spans[end].0
    };
    let text = script_lines[line_index][text_start..text_end].trim().to_string();

    let mut match_count = 0usize;
    let mut begin_ms: Option<u64> = None;
    let mut end_ms: Option<u64> = None;
    for k in start..end {
        if let Some(asr_idx) = hit_asr_index(&links[k]) {
            claimed[asr_idx] = true;
            let token = &asr_tokens[asr_idx];
            begin_ms = Some(begin_ms.map_or(token.begin_ms, |b: u64| b.min(token.begin_ms)));
            end_ms = Some(end_ms.map_or(token.end_ms, |e: u64| e.max(token.end_ms)));
        }
        if matches!(links[k], TokenLink::Match(_)) {
            match_count += 1;
        }
    }
    let begin_ms = begin_ms.expect("kept 段经过 reclassify_hitless_good_runs 保证至少一个命中");
    let end_ms = end_ms.expect("kept 段经过 reclassify_hitless_good_runs 保证至少一个命中");
    (
        OptimizedSegment::Script {
            line_index,
            text,
            begin_ms,
            end_ms,
            match_ratio: match_count as f32 / (end - start) as f32,
        },
        begin_ms,
    )
}

/// 扫描未被任何保留片段认领的 ASR token（按时间连续），时长达标的合并为一段
/// “识别插入”。既覆盖被丢弃坏段对应的音频，也覆盖夹在两个好段之间、文稿里
/// 完全没写但音频确实说了的即兴内容——两者对这一步是同一件事。
fn collect_orphan_segments(asr_tokens: &[AsrToken], claimed: &[bool]) -> Vec<(u64, OptimizedSegment)> {
    let mut out = Vec::new();
    let n = asr_tokens.len();
    let mut i = 0;
    while i < n {
        if claimed[i] {
            i += 1;
            continue;
        }
        let mut j = i;
        while j < n && !claimed[j] {
            j += 1;
        }
        let begin_ms = asr_tokens[i].begin_ms;
        let end_ms = asr_tokens[j - 1].end_ms;
        if end_ms.saturating_sub(begin_ms) >= MIN_ASR_INSERTION_MS {
            let word_begin = asr_tokens[i..j].iter().map(|t| t.word_index).min().unwrap();
            let word_end = asr_tokens[i..j].iter().map(|t| t.word_index).max().unwrap();
            out.push((begin_ms, OptimizedSegment::Asr { word_begin, word_end }));
        }
        i = j;
    }
    out
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
