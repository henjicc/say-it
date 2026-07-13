//! 字幕文档的纯领域规则；前端只保留拖拽、播放头和未提交草稿。
use serde::{Deserialize, Serialize};

const MIN_CUE_MS: i64 = 300;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SubtitleCue {
    pub(crate) id: String,
    pub(crate) begin_ms: i64,
    pub(crate) end_ms: i64,
    pub(crate) text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) speaker_id: Option<String>,
}

/// 排序并修复重叠/零时长，作为编辑 patch 和导出的共同校验边界。
pub(crate) fn normalize_timeline(mut cues: Vec<SubtitleCue>) -> Vec<SubtitleCue> {
    cues.retain(|cue| !cue.text.trim().is_empty());
    cues.sort_by(|a, b| a.begin_ms.cmp(&b.begin_ms).then(a.end_ms.cmp(&b.end_ms)));
    let mut previous_end = 0_i64;
    for cue in &mut cues {
        cue.text = cue.text.trim().to_string();
        cue.begin_ms = cue.begin_ms.max(0).max(previous_end);
        cue.end_ms = cue.end_ms.max(cue.begin_ms + MIN_CUE_MS);
        previous_end = cue.end_ms;
    }
    cues
}

pub(crate) fn to_srt(cues: Vec<SubtitleCue>) -> String {
    let cues = normalize_timeline(cues);
    cues.iter()
        .enumerate()
        .map(|(index, cue)| {
            let text = match &cue.speaker_id {
                Some(speaker_id) if !speaker_id.is_empty() => {
                    format!("说话人 {speaker_id}：{}", cue.text)
                }
                _ => cue.text.clone(),
            };
            format!(
                "{}\r\n{} --> {}\r\n{}",
                index + 1,
                format_srt_time(cue.begin_ms),
                format_srt_time(cue.end_ms),
                text
            )
        })
        .collect::<Vec<_>>()
        .join("\r\n\r\n")
        + "\r\n"
}

fn format_srt_time(value: i64) -> String {
    let value = value.max(0);
    let hours = value / 3_600_000;
    let minutes = (value % 3_600_000) / 60_000;
    let seconds = (value % 60_000) / 1_000;
    let milliseconds = value % 1_000;
    format!("{hours:02}:{minutes:02}:{seconds:02},{milliseconds:03}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cue(id: &str, begin_ms: i64, end_ms: i64, text: &str) -> SubtitleCue {
        SubtitleCue {
            id: id.into(),
            begin_ms,
            end_ms,
            text: text.into(),
            speaker_id: None,
        }
    }

    #[test]
    fn normalizes_overlaps_and_minimum_duration_before_export() {
        let output = to_srt(vec![
            cue("b", 500, 400, " 后一句 "),
            cue("a", -1, 600, "前一句"),
        ]);
        assert_eq!(output, "1\r\n00:00:00,000 --> 00:00:00,600\r\n前一句\r\n\r\n2\r\n00:00:00,600 --> 00:00:00,900\r\n后一句\r\n");
    }
}
