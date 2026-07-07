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

fn as_script(seg: &OptimizedSegment) -> (usize, &str, u64, u64, f32) {
    match seg {
        OptimizedSegment::Script { line_index, text, begin_ms, end_ms, match_ratio } => {
            (*line_index, text.as_str(), *begin_ms, *end_ms, *match_ratio)
        }
        OptimizedSegment::Asr { .. } => panic!("expected script segment, got asr"),
    }
}

fn as_asr(seg: &OptimizedSegment) -> (usize, usize) {
    match seg {
        OptimizedSegment::Asr { word_begin, word_end } => (*word_begin, *word_end),
        OptimizedSegment::Script { .. } => panic!("expected asr segment, got script"),
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
    let out = align_script(&words, &lines(&["今天天气很好", "明天再见"])).unwrap().lines;
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
    let out = align_script(&words, &lines(&["今天天气真的很好"])).unwrap().lines;
    assert_eq!(out[0].begin_ms, 0);
    assert_eq!(out[0].end_ms, 600);
    assert!(out[0].match_ratio < 1.0 && out[0].match_ratio >= 0.7);
    assert!(!out[0].interpolated);
}

#[test]
fn asr_fillers_are_skipped() {
    // ASR 里的语气词/口头语不拉偏行时间
    let words = char_words("嗯今天那个天气很好", 0, 100);
    let out = align_script(&words, &lines(&["今天天气很好"])).unwrap().lines;
    assert_eq!(out[0].begin_ms, 100);
    assert_eq!(out[0].end_ms, 900);
    assert!((out[0].match_ratio - 1.0).abs() < f32::EPSILON);
}

#[test]
fn substitution_keeps_timing_and_lowers_ratio() {
    // 识别错字（替换对）不影响行时间，但拉低匹配率
    let words = char_words("今天天汽很好", 0, 100);
    let out = align_script(&words, &lines(&["今天天气很好"])).unwrap().lines;
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
    .unwrap()
    .lines;
    assert!(out[1].interpolated);
    assert!(out[1].match_ratio < 0.3);
    assert!(out[1].begin_ms >= out[0].end_ms);
    assert!(out[1].end_ms <= out[2].begin_ms);
    assert_timeline_valid(&out);
}

#[test]
fn mixed_cjk_latin() {
    let words = vec![w("我用", 0, 600), w("github", 600, 1200), w("写代码", 1200, 1800)];
    let out = align_script(&words, &lines(&["我用 GitHub 写代码"])).unwrap().lines;
    assert_eq!((out[0].begin_ms, out[0].end_ms), (0, 1800));
    assert!((out[0].match_ratio - 1.0).abs() < f32::EPSILON);
}

#[test]
fn chinese_digits_match_arabic() {
    let words = char_words("二零二四年发布", 0, 100);
    let out = align_script(&words, &lines(&["2024年发布"])).unwrap().lines;
    assert!((out[0].match_ratio - 1.0).abs() < f32::EPSILON);
    assert_eq!((out[0].begin_ms, out[0].end_ms), (0, 700));
}

#[test]
fn min_duration_is_enforced() {
    let words = vec![w("好", 1000, 1050)];
    let out = align_script(&words, &lines(&["好"])).unwrap().lines;
    assert_eq!(out[0].begin_ms, 1000);
    assert!(out[0].end_ms - out[0].begin_ms >= MIN_LINE_DURATION_MS);
}

#[test]
fn leading_audio_junk_is_free() {
    // 片头与文稿无关的内容不产生罚分，也不拉偏第一行时间（半全局对齐）
    let words = char_words("废话闲聊几句吧正文从这里开始", 0, 100);
    let out = align_script(&words, &lines(&["正文从这里开始"])).unwrap().lines;
    assert_eq!(out[0].begin_ms, 700);
    assert!((out[0].match_ratio - 1.0).abs() < f32::EPSILON);
}

#[test]
fn empty_inputs() {
    assert!(align_script(&[], &lines(&["你好"])).is_err());
    assert!(align_script(&[w("你好", 0, 100)], &[]).unwrap().lines.is_empty());
}

#[test]
fn blank_line_gets_zero_width_slot() {
    let words = char_words("今天天气很好明天再见", 0, 100);
    let out = align_script(&words, &lines(&["今天天气很好", "", "明天再见"])).unwrap().lines;
    assert!(out[1].interpolated);
    assert_eq!(out[1].begin_ms, out[1].end_ms);
    assert_timeline_valid(&out);
}

#[test]
fn large_input_uses_anchors() {
    // 超过全矩阵对齐规模上限，走锚点分治路径
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
    let out = align_script(&words, &script).unwrap().lines;
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
    let out = align_script(&words, &script).unwrap().lines;
    assert!(out.iter().all(|l| l.match_ratio == 0.0));
    assert_timeline_valid(&out);
}

#[test]
fn optimized_matches_script_when_fully_aligned() {
    let words = vec![
        w("今天", 0, 600),
        w("天气", 600, 1200),
        w("很好", 1200, 1800),
        w("明天", 2000, 2600),
        w("再见", 2600, 3200),
    ];
    let out = align_script(&words, &lines(&["今天天气很好", "明天再见"])).unwrap();
    assert_eq!(out.optimized_segments.len(), 2);
    let (line0, text0, begin0, end0, ratio0) = as_script(&out.optimized_segments[0]);
    assert_eq!((line0, text0, begin0, end0), (0, "今天天气很好", 0, 1800));
    assert!((ratio0 - 1.0).abs() < f32::EPSILON);
    let (line1, text1, begin1, end1, _) = as_script(&out.optimized_segments[1]);
    assert_eq!((line1, text1, begin1, end1), (1, "明天再见", 2000, 3200));
}

#[test]
fn optimized_keeps_line_on_isolated_substitution() {
    // 单个误听字（长度 1，低于阈值）不足以触发替换，整行仍原样保留
    let words = char_words("今天天汽很好", 0, 100);
    let out = align_script(&words, &lines(&["今天天气很好"])).unwrap();
    assert_eq!(out.optimized_segments.len(), 1);
    let (_, text, begin, end, ratio) = as_script(&out.optimized_segments[0]);
    assert_eq!((text, begin, end), ("今天天气很好", 0, 600));
    assert!(ratio < 1.0);
}

#[test]
fn optimized_drops_unspoken_line_with_nothing_to_fill() {
    // 中间一行文稿写了但音频完全没说：既不保留该行文本，也没有可填充的音频（正确丢弃）
    let mut words = char_words("第一句话说完了", 0, 100);
    words.extend(char_words("第三句话开始了", 2000, 100));
    let out = align_script(
        &words,
        &lines(&["第一句话说完了", "完全无关的内容啊", "第三句话开始了"]),
    )
    .unwrap();
    assert_eq!(out.optimized_segments.len(), 2);
    assert_eq!(as_script(&out.optimized_segments[0]).0, 0);
    assert_eq!(as_script(&out.optimized_segments[1]).0, 2);
}

#[test]
fn optimized_splits_line_on_long_internal_mismatch() {
    // 一行内部有一段足够长（>=4 token）的内容音频里完全没有，其余部分正常匹配：
    // 应该拆成“保留头部”+“保留尾部”，中间因音频无对应内容而彻底消失（无可填充）
    let words = char_words("开头正确结尾正确", 0, 100);
    let out = align_script(&words, &lines(&["开头正确这段完全不存在结尾正确"])).unwrap();
    assert_eq!(out.optimized_segments.len(), 2);
    let (line0, text0, begin0, end0, _) = as_script(&out.optimized_segments[0]);
    assert_eq!((line0, text0, begin0, end0), (0, "开头正确", 0, 400));
    let (line1, text1, begin1, end1, _) = as_script(&out.optimized_segments[1]);
    assert_eq!((line1, text1, begin1, end1), (0, "结尾正确", 400, 800));
}

#[test]
fn optimized_inserts_pure_audio_only_content() {
    // 两行文稿各自与音频完全吻合，但音频中间还说了一段文稿里完全没有的内容：
    // 应该在两个保留段之间插入一段识别文本
    let audio_text = "开头完全一致这里是文稿没有写的额外插入内容结尾完全一致";
    let words = char_words(audio_text, 0, 100);
    let out = align_script(&words, &lines(&["开头完全一致", "结尾完全一致"])).unwrap();
    let segs = out.optimized_segments;
    assert_eq!(segs.len(), 3, "{segs:?}");
    assert_eq!(as_script(&segs[0]).1, "开头完全一致");
    let (word_begin, word_end) = as_asr(&segs[1]);
    assert!(word_end >= word_begin);
    assert_eq!(as_script(&segs[2]).1, "结尾完全一致");
}

#[test]
fn optimized_skips_tiny_orphan_gap() {
    // 两行之间只有极短的未认领音频（远低于 500ms 阈值），不应产出识别插入段
    let audio_text = "开头完全一致啊结尾完全一致";
    let words = char_words(audio_text, 0, 100);
    let out = align_script(&words, &lines(&["开头完全一致", "结尾完全一致"])).unwrap();
    assert_eq!(out.optimized_segments.len(), 2);
}

#[test]
fn optimized_merges_replacement_across_line_boundary() {
    // 连续两行文稿音频里说的是完全不同的内容（且各自都不短），应合并为一段识别插入，
    // 而不是分别产出两段
    let head = "开场白导入语";
    let real_middle: String = (0..30u32).map(|i| char::from_u32(0x9000 + i).unwrap()).collect();
    let tail = "结束语收尾";
    let audio_text = format!("{head}{real_middle}{tail}");
    let words = char_words(&audio_text, 0, 100);
    let fake_line_a: String = (0..15u32).map(|i| char::from_u32(0x6000 + i).unwrap()).collect();
    let fake_line_b: String = (0..15u32).map(|i| char::from_u32(0x6100 + i).unwrap()).collect();
    let out = align_script(&words, &lines(&[head, &fake_line_a, &fake_line_b, tail])).unwrap();
    let segs = out.optimized_segments;
    assert_eq!(segs.len(), 3, "{segs:?}");
    assert_eq!(as_script(&segs[0]).1, head);
    let (word_begin, word_end) = as_asr(&segs[1]);
    assert_eq!(word_end - word_begin + 1, real_middle.chars().count());
    assert_eq!(as_script(&segs[2]).1, tail);
}

#[test]
fn optimized_blank_line_produces_no_segment() {
    let words = char_words("今天天气很好明天再见", 0, 100);
    let out = align_script(&words, &lines(&["今天天气很好", "", "明天再见"])).unwrap();
    let line_indices: Vec<usize> = out
        .optimized_segments
        .iter()
        .filter_map(|seg| match seg {
            OptimizedSegment::Script { line_index, .. } => Some(*line_index),
            OptimizedSegment::Asr { .. } => None,
        })
        .collect();
    assert_eq!(line_indices, vec![0, 2]);
}

#[test]
fn optimized_empty_script_has_no_segments() {
    let out = align_script(&[w("你好", 0, 100)], &[]).unwrap();
    assert!(out.optimized_segments.is_empty());
}
