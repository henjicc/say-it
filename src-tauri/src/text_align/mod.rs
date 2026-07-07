//! 文稿对齐纯算法：把 ASR 词级时间戳映射到用户文稿行，产出行级字幕时间轴，
//! 并在此基础上生成一份“智能修正”结果。本模块不依赖 tauri，可独立单元测试。
//!
//! 思路（forced alignment 的文本域近似）：
//! 1. 两侧文本规整成 token 序列（CJK 单字、拉丁连串、数字逐字符，中文数字字符等价为阿拉伯数字）；
//! 2. 半全局仿射 gap 对齐（Gotoh 三状态，ASR 侧首尾 gap 免罚，容忍音频里存在文稿之外的
//!    片头/片尾内容）：小段直接跑满矩阵，大段先用唯一 n-gram 锚点分治，无锚点时带宽 DP 兜底；
//! 3. “完全按文稿”结果：每行时间取该行命中 token（匹配或替换对）的首尾时间，
//!    命中太少的行按相邻行内插；
//! 4. “识别修正”结果：不再以整行为最小单位。文稿 token 按连续坏段（长度 ≥ 阈值的
//!    非匹配 token 串）拆出需要丢弃的片段，保留片段仍取原文稿文本；同时，凡是没有被
//!    任何保留片段认领的 ASR 词（无论是坏段自身对应的音频，还是夹在两个完全匹配片段
//!    之间、文稿压根没写的即兴内容）都会被收集为“识别插入”片段一并输出，按时间与
//!    保留片段交织排列。
//! 5. 后处理保证时间轴单调、不重叠、非空行不短于最小时长。

mod align;
mod segments;
mod tokenize;

use serde::{Deserialize, Serialize};

/// 每条字幕的最小展示时长。
pub const MIN_LINE_DURATION_MS: u64 = 300;

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

/// 对齐输出的行级字幕（“完全按文稿”结果，文本 100% 等于用户文稿）。
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

/// “识别修正”结果的一个片段：要么原样保留文稿某一行的（部分）文本，
/// 要么是一段未被文稿认领的音频，需要由调用方按词范围重建识别文本与时间
/// （复用现有的字幕切分逻辑，故这里只给词范围，不重复生成文本）。
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "source", rename_all = "lowercase")]
pub enum OptimizedSegment {
    #[serde(rename_all = "camelCase")]
    Script {
        line_index: usize,
        text: String,
        begin_ms: u64,
        end_ms: u64,
        match_ratio: f32,
    },
    #[serde(rename_all = "camelCase")]
    Asr {
        /// 输入 words 的下标范围（含两端）。
        word_begin: usize,
        word_end: usize,
    },
}

/// `align_script` 的完整输出：“完全按文稿”与“识别修正”两份结果一次算出，
/// 避免重复跑一遍对齐。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlignOutput {
    pub lines: Vec<AlignedLine>,
    pub optimized_segments: Vec<OptimizedSegment>,
}

/// 对齐主入口：`script_lines` 一行一句，输出与输入行一一对应的“完全按文稿”结果，
/// 以及片段化的“识别修正”结果。
pub fn align_script(words: &[AlignWord], script_lines: &[String]) -> Result<AlignOutput, String> {
    if script_lines.is_empty() {
        return Ok(AlignOutput { lines: Vec::new(), optimized_segments: Vec::new() });
    }
    let asr_tokens = tokenize::build_asr_tokens(words);
    if asr_tokens.is_empty() {
        return Err("识别结果中没有可用的词级时间戳，无法对齐".to_string());
    }

    let mut script_texts: Vec<String> = Vec::new();
    let mut token_spans: Vec<(usize, usize)> = Vec::new();
    let mut line_ranges: Vec<(usize, usize)> = Vec::with_capacity(script_lines.len());
    for line in script_lines {
        let start = script_texts.len();
        for (token, span_start, span_end) in tokenize::tokenize_with_spans(line) {
            script_texts.push(token);
            token_spans.push((span_start, span_end));
        }
        line_ranges.push((start, script_texts.len()));
    }

    let (script_ids, asr_ids) = align::intern_ids(&script_texts, &asr_tokens);
    let links = align::align_tokens(&script_ids, &asr_ids);

    let lines = segments::build_aligned_lines(&links, &line_ranges, script_lines, &asr_tokens);
    let optimized_segments = segments::build_optimized_segments(
        &links,
        &line_ranges,
        &token_spans,
        script_lines,
        &asr_tokens,
    );

    Ok(AlignOutput { lines, optimized_segments })
}

#[cfg(test)]
mod tests;
