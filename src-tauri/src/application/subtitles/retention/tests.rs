use super::super::*;

fn legacy_commit(doc: &mut TranslationDocument, mode: &str, continuing: bool) {
    let group = std::mem::take(&mut doc.current_group);
    doc.committed_groups.push(group.clone());
    if doc.committed_groups.len() > 12 {
        doc.committed_groups.remove(0);
    }
    if mode == "replace" {
        if continuing {
            doc.replace_groups.push(group);
        } else {
            doc.replace_groups = vec![group];
        }
    }
    doc.partial_offset = 0;
}

fn same_display(doc: &TranslationDocument, legacy: &TranslationDocument) {
    for (mode, lines) in [("replace", 1), ("scroll", 1), ("scroll", 3), ("scroll", 12)] {
        let prefs = SubtitlePrefs {
            mode: mode.into(),
            line_count: lines,
            ..SubtitlePrefs::default()
        };
        assert_eq!(
            doc.display(&prefs),
            legacy.display_legacy(&prefs),
            "mode={mode}, lines={lines}"
        );
    }
}

#[test]
fn rendering_and_retention_match_old_output_with_out_of_order_streams() {
    let mut doc = TranslationDocument::default();
    let mut legacy = TranslationDocument::default();
    let mut pending = Vec::new();
    for index in 0..160 {
        let source = "第一句。第二句。尾句";
        let dispatched = doc.dispatch(source, true);
        assert_eq!(dispatched, legacy.dispatch(source, true));
        for (seq, _) in dispatched {
            let long_partial = format!("  {} {seq}", " 中🙂\t".repeat(500));
            doc.update(seq, long_partial.clone());
            legacy.values.insert(seq, long_partial);
            same_display(&doc, &legacy);
            pending.push(seq);
        }
        let mode = if index % 19 == 0 { "scroll" } else { "replace" };
        let continuing = index % 17 != 0;
        doc.commit(mode, continuing);
        legacy_commit(&mut legacy, mode, continuing);
        same_display(&doc, &legacy);
        // 完成顺序与文本长度都可能变化；旧请求拖到后面才完成。
        if pending.len() > 8 {
            let seq = pending.remove(pending.len() - 3);
            let text = if seq % 3 == 0 {
                String::new()
            } else {
                format!("译{seq}{}", "🙂 汉\n".repeat(90))
            };
            doc.update(seq, text.clone());
            doc.finish(seq);
            legacy.values.insert(seq, text);
            same_display(&doc, &legacy);
        }
    }
    for seq in pending.into_iter().rev() {
        let text = format!("  最终{seq}{}", "中文🙂 ".repeat(160));
        doc.update(seq, text.clone());
        doc.finish(seq);
        legacy.values.insert(seq, text);
        same_display(&doc, &legacy);
    }
}

#[test]
fn completed_history_stays_bounded_and_late_events_cannot_restore_it() {
    for mode in ["scroll", "replace"] {
        let mut doc = TranslationDocument::default();
        for _ in 0..4_000 {
            let seq = doc.dispatch("一句", true)[0].0;
            doc.update(seq, "字".repeat(300));
            doc.finish(seq);
            doc.commit(mode, true);
        }
        assert!(doc.values.len() <= 12, "{mode}: {}", doc.values.len());
        assert!(doc.replace_groups.len() <= 7);
        assert!(!doc.update(1, "迟到旧结果".into()));
        assert!(!doc.values.contains_key(&1));
    }
}

#[test]
fn a_single_uncommitted_sentence_and_empty_results_are_reclaimed() {
    let mut doc = TranslationDocument::default();
    let mut text = String::new();
    for index in 0..2_000 {
        text.push_str("一句。");
        for (seq, _) in doc.dispatch(&text, false) {
            doc.update(
                seq,
                if index % 2 == 0 {
                    "译".repeat(300)
                } else {
                    String::new()
                },
            );
            doc.finish(seq);
        }
        assert!(doc.current_group.len() <= 7);
        assert!(doc.values.len() <= 7);
    }
}

#[test]
fn a_shorter_final_can_reveal_older_text_until_the_suffix_is_stable() {
    let mut doc = TranslationDocument::default();
    let old = doc.dispatch("旧句", true)[0].0;
    doc.update(old, "旧译文".into());
    doc.finish(old);
    doc.commit("replace", false);
    let new = doc.dispatch("新句", true)[0].0;
    doc.update(new, "临时".repeat(2_000));
    doc.commit("replace", true);
    assert!(doc.values.contains_key(&old));
    doc.update(new, "短最终".into());
    doc.finish(new);
    assert_eq!(doc.display(&SubtitlePrefs::default()), "旧译文 短最终");
    assert!(!doc.update(new, "完成后重复事件".into()));
}

#[test]
fn clipping_preserves_whitespace_and_character_boundaries() {
    for size in [0, 1, 1799, 1800, 1801, 1802, 4000] {
        for prefix in [" ", "\n\t", "🙂"] {
            let text = format!("{prefix}{}", "字".repeat(size));
            let mut doc = TranslationDocument::default();
            let seq = doc.dispatch("句", true)[0].0;
            doc.update(seq, text.clone());
            assert_eq!(
                doc.display(&SubtitlePrefs::default()),
                super::super::tail_chars(&text, MAX_TEXT_CHARS)
            );
        }
    }
}

#[test]
#[ignore = "独立字幕长会话测量；不调用设备或服务"]
fn long_session_profile() {
    use crate::performance_test_support::{memory, thread_cycles};
    let legacy = std::env::var("SAYIT_PERF_SUBTITLE_LEGACY").as_deref() == Ok("1");
    let mut doc = TranslationDocument::default();
    let prefs = SubtitlePrefs::default();
    let initial = memory();
    let cycles = thread_cycles();
    let started = std::time::Instant::now();
    let mut output_hash = 0xcbf29ce484222325u64;
    for index in 0..4_000 {
        let seq = doc.dispatch("本地测试。", true)[0].0;
        let text = format!("字幕{index}{}", "汉🙂".repeat(256));
        if legacy {
            doc.values.insert(seq, text);
            legacy_commit(&mut doc, "replace", true);
        } else {
            doc.update(seq, text);
            doc.finish(seq);
            doc.commit("replace", true);
        }
        let displayed = if legacy {
            doc.display_legacy(&prefs)
        } else {
            doc.display(&prefs)
        };
        for byte in displayed.bytes() {
            output_hash = (output_hash ^ byte as u64).wrapping_mul(0x100000001b3);
        }
    }
    let elapsed = started.elapsed().as_secs_f64() * 1_000.0;
    let cycles = thread_cycles() - cycles;
    let after = memory();
    println!(
        "PERF_RESULT {}",
        serde_json::json!({
            "scenario":"subtitle-retention", "legacy":legacy,"segments":4000,
            "retainedResults":doc.values.len(), "retainedReplaceGroups":doc.replace_groups.len(),
            "elapsedMs":elapsed,"threadCycles":cycles,"outputHash":format!("{output_hash:016x}"),
            "initialPrivateBytes":initial.private_usage,"finalPrivateBytes":after.private_usage,
            "peakPrivateBytes":after.peak_pagefile_usage,
        })
    );
}
