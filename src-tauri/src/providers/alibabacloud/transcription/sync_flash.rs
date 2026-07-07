use serde_json::Value;

use super::types::{TranscriptionResult, TranscriptionSentence, TranscriptionTranscript};

/// 解析 fun-asr-flash 的 SSE 流式响应。每个事件的 `data:` 行是一个独立 JSON 对象；
/// 当 `output.sentence.sentence_end` 为 true 时该句已定稿，把它累积成一个
/// [`TranscriptionSentence`]（含逐词时间戳）。最后一个事件的 `output.text` 是整段音频的
/// 完整识别文本，直接作为 transcript 的 text。
pub(super) fn parse_fun_asr_flash_sse(text: &str) -> Result<TranscriptionResult, String> {
    let mut sentences: Vec<TranscriptionSentence> = Vec::new();
    let mut final_text = String::new();
    let mut duration_ms: Option<u64> = None;
    let mut channel_id: Option<Value> = None;
    let mut saw_event = false;

    for line in text.lines() {
        let Some(data) = line.trim().strip_prefix("data:") else {
            continue;
        };
        let data = data.trim();
        if data.is_empty() {
            continue;
        }
        let Ok(event) = serde_json::from_str::<Value>(data) else {
            continue;
        };
        saw_event = true;

        if let Some(text) = event.pointer("/output/text").and_then(Value::as_str) {
            final_text = text.to_string();
        }
        if let Some(seconds) = event.pointer("/usage/duration").and_then(Value::as_u64) {
            duration_ms = Some(seconds.saturating_mul(1000));
        }

        let Some(sentence_val) = event.pointer("/output/sentence") else {
            continue;
        };
        let sentence_end = sentence_val
            .get("sentence_end")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if !sentence_end {
            continue;
        }
        if channel_id.is_none() {
            channel_id = sentence_val.get("channel_id").cloned();
        }
        let sentence: TranscriptionSentence = serde_json::from_value(sentence_val.clone())
            .map_err(|e| format!("解析录音识别句子失败：{e}"))?;
        // fun-asr-flash 真实返回里可能反复把“同一句正在定稿中的整句”重发为 sentence_end=true。
        // 已观察到三种变体：同一个 sentence_id 重发；sentence_id 变化但 begin_time/channel
        // 不变、文本只是向后增长；上一句刚定稿完，紧接着又原样/前缀重发一遍且 begin_time
        // 顺移到了上一句的 end_time。前两种应覆盖上一条，第三种应只把时间范围并入上一条，
        // 保留上一条本身更准确的文本与逐词时间戳，而不是追加成多条重复字幕。
        match sentences.last_mut() {
            Some(last) if last_sentence_still_finalizing(last, &sentence) => *last = sentence,
            Some(last) if next_sentence_is_stale_echo(last, &sentence) => {
                last.end_time = last.end_time.max(sentence.end_time);
            }
            _ => sentences.push(sentence),
        }
    }

    if !saw_event {
        return Err("短音频识别响应为空或格式不正确".to_string());
    }
    if final_text.trim().is_empty() && sentences.is_empty() {
        return Err("短音频识别成功但响应里没有可用文本".to_string());
    }

    Ok(TranscriptionResult {
        duration_ms,
        transcripts: vec![TranscriptionTranscript {
            channel_id,
            text: final_text,
            sentences,
        }],
    })
}

/// 同一句仍在“继续定稿”：begin_time 不变，文本只是向后增长（或反复重发同一整句）。
fn last_sentence_still_finalizing(
    last: &TranscriptionSentence,
    next: &TranscriptionSentence,
) -> bool {
    if last.sentence_id.is_some() && last.sentence_id == next.sentence_id {
        return true;
    }

    if last.begin_time != next.begin_time || last.end_time > next.end_time {
        return false;
    }
    if last.speaker_id != next.speaker_id {
        return false;
    }

    is_duplicate_sentence_text(&last.text, &next.text)
}

/// 上一句刚定稿完（begin_time != next.begin_time），紧接着又原样/前缀重发了一遍，
/// 且这次的 begin_time 顺移到了上一句的 end_time——是同一句的过期回声，而不是新句子。
fn next_sentence_is_stale_echo(last: &TranscriptionSentence, next: &TranscriptionSentence) -> bool {
    if next.begin_time != last.end_time {
        return false;
    }
    if last.speaker_id != next.speaker_id {
        return false;
    }

    is_duplicate_sentence_text(&last.text, &next.text)
}

fn is_duplicate_sentence_text(last_text: &str, next_text: &str) -> bool {
    let last_text = last_text.trim();
    let next_text = next_text.trim();
    if last_text.is_empty() || next_text.is_empty() {
        return false;
    }

    next_text == last_text
        || next_text.starts_with(last_text)
        || last_text.starts_with(next_text)
}

pub(super) fn extract_sse_error_message(text: &str) -> String {
    match serde_json::from_str::<Value>(text) {
        Ok(value) => super::http::extract_error_message(&value, text),
        Err(_) => super::http::truncate(text, 200),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// 取自文档《Fun-ASR录音文件识别HTTP API参考.md》里 fun-asr-flash 流式响应的原样示例。
    const FUN_ASR_FLASH_SSE_SAMPLE: &str = concat!(
        "id:1\n",
        "event:result\n",
        ":HTTP_STATUS/200\n",
        "data:{\"output\":{\"sentence\":{\"sentence_id\":1,\"sentence_end\":true,\"end_time\":3800,",
        "\"words\":[",
        "{\"end_time\":1040,\"punctuation\":\"\",\"begin_time\":760,\"fixed\":true,\"text\":\"Hello\"},",
        "{\"end_time\":1240,\"punctuation\":\"，\",\"begin_time\":1040,\"fixed\":true,\"text\":\" World\"},",
        "{\"end_time\":1880,\"punctuation\":\"\",\"begin_time\":1360,\"fixed\":true,\"text\":\"这里是\"},",
        "{\"end_time\":2520,\"punctuation\":\"\",\"begin_time\":1880,\"fixed\":true,\"text\":\"阿里巴巴\"},",
        "{\"end_time\":2840,\"punctuation\":\"\",\"begin_time\":2520,\"fixed\":true,\"text\":\"语音\"},",
        "{\"end_time\":3800,\"punctuation\":\"。\",\"begin_time\":2840,\"fixed\":true,\"text\":\"实验室\"}",
        "],\"begin_time\":760,\"text\":\"Hello World，这里是阿里巴巴语音实验室。\",\"channel_id\":0},",
        "\"text\":\"Hello World，这里是阿里巴巴语音实验室。\"},",
        "\"usage\":{\"duration\":4},",
        "\"request_id\":\"fc1582e4-935c-9fc2-a482-a98bf43daa69\"}\n",
        "\n",
    );

    #[test]
    fn parses_fun_asr_flash_sse_sample_from_docs() {
        let result = parse_fun_asr_flash_sse(FUN_ASR_FLASH_SSE_SAMPLE)
            .expect("doc sample should parse");
        assert_eq!(result.duration_ms, Some(4000));
        assert_eq!(result.transcripts.len(), 1);
        let transcript = &result.transcripts[0];
        assert_eq!(transcript.text, "Hello World，这里是阿里巴巴语音实验室。");
        assert_eq!(transcript.channel_id, Some(json!(0)));
        assert_eq!(transcript.sentences.len(), 1);
        let sentence = &transcript.sentences[0];
        assert_eq!(sentence.begin_time, 760);
        assert_eq!(sentence.end_time, 3800);
        assert_eq!(sentence.words.len(), 6);
        assert_eq!(sentence.words[0].text, "Hello");
        assert_eq!(sentence.words[5].text, "实验室");
        assert_eq!(sentence.words[5].punctuation.as_deref(), Some("。"));
    }

    /// 同一个 sentence_id 在稳定过程中反复以 sentence_end=true 重发（每加稳一个词就整句重发一次），
    /// 应只保留每个 sentence_id 的最后一条，而不是把每次重发都当成新句子。
    #[test]
    fn dedups_repeated_sentence_end_events_for_same_sentence_id() {
        let events = concat!(
            "data:{\"output\":{\"sentence\":{\"sentence_id\":1,\"sentence_end\":true,",
            "\"begin_time\":4200,\"end_time\":4500,\"text\":\"那为什么\",",
            "\"words\":[{\"begin_time\":4200,\"end_time\":4500,\"text\":\"那为什么\",\"punctuation\":\"\"}]},",
            "\"text\":\"那为什么\"},\"request_id\":\"r1\"}\n",
            "\n",
            "data:{\"output\":{\"sentence\":{\"sentence_id\":1,\"sentence_end\":true,",
            "\"begin_time\":4200,\"end_time\":5400,\"text\":\"那为什么这些格式转换APP要么一堆广告\",",
            "\"words\":[{\"begin_time\":4200,\"end_time\":5400,\"text\":\"那为什么这些格式转换APP要么一堆广告\",\"punctuation\":\"\"}]},",
            "\"text\":\"那为什么这些格式转换APP要么一堆广告\"},\"request_id\":\"r1\"}\n",
            "\n",
            "data:{\"output\":{\"sentence\":{\"sentence_id\":2,\"sentence_end\":true,",
            "\"begin_time\":5400,\"end_time\":7400,\"text\":\"我就寻思着\",",
            "\"words\":[{\"begin_time\":5400,\"end_time\":7400,\"text\":\"我就寻思着\",\"punctuation\":\"\"}]},",
            "\"text\":\"那为什么这些格式转换APP要么一堆广告 我就寻思着\"},",
            "\"usage\":{\"duration\":7},\"request_id\":\"r1\"}\n",
            "\n",
        );
        let result = parse_fun_asr_flash_sse(events).expect("should parse");
        let transcript = &result.transcripts[0];
        assert_eq!(
            transcript.sentences.len(),
            2,
            "repeated sentence_end for the same sentence_id must collapse into one entry"
        );
        assert_eq!(
            transcript.sentences[0].text,
            "那为什么这些格式转换APP要么一堆广告"
        );
        assert_eq!(transcript.sentences[0].end_time, 5400);
        assert_eq!(transcript.sentences[1].text, "我就寻思着");
    }

    #[test]
    fn dedups_repeated_sentence_end_events_even_if_sentence_id_changes() {
        let events = concat!(
            "data:{\"output\":{\"sentence\":{\"sentence_id\":11,\"sentence_end\":true,",
            "\"channel_id\":0,\"begin_time\":4200,\"end_time\":4500,\"text\":\"那为什么\",",
            "\"words\":[{\"begin_time\":4200,\"end_time\":4500,\"text\":\"那为什么\",\"punctuation\":\"\"}]},",
            "\"text\":\"那为什么\"},\"request_id\":\"r1\"}\n",
            "\n",
            "data:{\"output\":{\"sentence\":{\"sentence_id\":12,\"sentence_end\":true,",
            "\"channel_id\":0,\"begin_time\":4200,\"end_time\":5400,\"text\":\"那为什么这些格式转换APP要么一堆广告\",",
            "\"words\":[{\"begin_time\":4200,\"end_time\":5400,\"text\":\"那为什么这些格式转换APP要么一堆广告\",\"punctuation\":\"\"}]},",
            "\"text\":\"那为什么这些格式转换APP要么一堆广告\"},\"request_id\":\"r1\"}\n",
            "\n",
            "data:{\"output\":{\"sentence\":{\"sentence_id\":13,\"sentence_end\":true,",
            "\"channel_id\":0,\"begin_time\":5400,\"end_time\":7400,\"text\":\"我就寻思着\",",
            "\"words\":[{\"begin_time\":5400,\"end_time\":7400,\"text\":\"我就寻思着\",\"punctuation\":\"\"}]},",
            "\"text\":\"那为什么这些格式转换APP要么一堆广告 我就寻思着\"},",
            "\"usage\":{\"duration\":7},\"request_id\":\"r1\"}\n",
            "\n",
        );
        let result = parse_fun_asr_flash_sse(events).expect("should parse");
        let transcript = &result.transcripts[0];
        assert_eq!(
            transcript.sentences.len(),
            2,
            "adjacent final events with same begin/channel and growing text must collapse"
        );
        assert_eq!(
            transcript.sentences[0].text,
            "那为什么这些格式转换APP要么一堆广告"
        );
        assert_eq!(transcript.sentences[0].end_time, 5400);
        assert_eq!(transcript.sentences[1].text, "我就寻思着");
    }

    /// 上一句刚定稿完（begin_time 不同），紧接着原样重发一遍且 begin_time 顺移到了
    /// 上一句的 end_time——应把这段时间并入上一句，而不是追加成一条极短的重复字幕。
    #[test]
    fn dedups_stale_echo_sentence_whose_begin_time_shifts_to_previous_end_time() {
        let events = concat!(
            "data:{\"output\":{\"sentence\":{\"sentence_id\":4,\"sentence_end\":true,",
            "\"begin_time\":4940,\"end_time\":7220,",
            "\"text\":\"那怎么用手机拍出这样的效果呢？哎，非常简单。\",",
            "\"words\":[{\"begin_time\":4940,\"end_time\":7220,",
            "\"text\":\"那怎么用手机拍出这样的效果呢？哎，非常简单。\",\"punctuation\":\"\"}]},",
            "\"text\":\"那怎么用手机拍出这样的效果呢？哎，非常简单。\"},\"request_id\":\"r1\"}\n",
            "\n",
            "data:{\"output\":{\"sentence\":{\"sentence_id\":5,\"sentence_end\":true,",
            "\"begin_time\":7220,\"end_time\":7520,",
            "\"text\":\"那怎么用手机拍出这样的效果呢？哎，非常简单。\",",
            "\"words\":[{\"begin_time\":7220,\"end_time\":7520,",
            "\"text\":\"那怎么用手机拍出这样的效果呢？哎，非常简单。\",\"punctuation\":\"\"}]},",
            "\"text\":\"那怎么用手机拍出这样的效果呢？哎，非常简单。\"},",
            "\"usage\":{\"duration\":8},\"request_id\":\"r1\"}\n",
            "\n",
        );
        let result = parse_fun_asr_flash_sse(events).expect("should parse");
        let transcript = &result.transcripts[0];
        assert_eq!(
            transcript.sentences.len(),
            1,
            "stale echo sentence with shifted begin_time must merge into the previous sentence"
        );
        assert_eq!(transcript.sentences[0].begin_time, 4940);
        assert_eq!(transcript.sentences[0].end_time, 7520);
        assert_eq!(
            transcript.sentences[0].text,
            "那怎么用手机拍出这样的效果呢？哎，非常简单。"
        );
    }
}
