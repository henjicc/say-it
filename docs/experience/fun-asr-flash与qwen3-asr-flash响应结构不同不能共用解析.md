# fun-asr-flash 与 qwen3-asr-flash 响应结构完全不同，不能共用一套解析逻辑

## 现象

修好 Opus/MP3 压缩上传之后，fun-asr-flash 识别报错：

```
识别失败。短音频识别响应缺少 output.choices[0].message.content
```

qwen3-asr-flash 能正常识别出文字，但没有逐词时间戳，字幕编辑器里显示不出分句/分词。

## 根因

虽然 fun-asr-flash-2026-06-15 和 qwen3-asr-flash 都是同步调用 `multimodal-generation` 接口，
但两者的**返回体结构完全不同**：

- **qwen3-asr-flash**：走的是标准 chat-completion 式结构，`output.choices[0].message.content[].text`，
  **不包含任何时间戳字段**（文档从请求示例到 Python SDK 的流式读取代码 `response["output"]["choices"][0]["message"].content[0]["text"]`
  全程都只有文本，没有 sentence/words）。这是这个模型 API 本身的能力边界，不是我们代码的 bug。
- **fun-asr-flash-2026-06-15**：走的是完全不同的结构（见
  `docs/API/阿里云百炼/Fun-ASR录音文件识别HTTP API参考.md` 第 213 行）：
  ```json
  {
    "output": {
      "text": "累积的完整文本",
      "sentence": {
        "sentence_id": 1, "sentence_end": true,
        "begin_time": 760, "end_time": 3800, "text": "...",
        "words": [{"begin_time":..,"end_time":..,"text":"..","punctuation":".."}]
      }
    },
    "usage": {"duration": 4}
  }
  ```
  这里根本没有 `choices` 字段，之前的代码统一用 `parse_short_audio_result` 去读
  `output.choices[0].message.content`，对 fun-asr-flash 必然找不到，直接报错。

进一步地，**非流式模式下 fun-asr-flash 只返回“最后一句”的 sentence.words**——`output.text` 是
全量累积文本，但 `output.sentence` 只是当前（最后）一句的详情。对于包含多个停顿分句的音频，
非流式请求拿不到完整的逐句/逐词时间戳。文档在"SSE 流式结果处理逻辑"一节明确说明，需要开启
`X-DashScope-SSE: enable`，对每个 `sentence_end: true` 事件分别取用其 `sentence` 累积成完整列表，
才能拿到覆盖全篇的时间戳。

## 修复

`src-tauri/src/providers/alibabacloud/transcription.rs`：

- fun-asr-flash 的请求加上 `X-DashScope-SSE: enable`，改为读取完整响应文本（`resp.text()`）
  按 SSE 格式解析（按行找 `data:` 前缀，逐个 JSON 解析），对每个 `sentence_end == true` 的事件
  累积成一个 `TranscriptionSentence`（含 `words`），最后一个事件的 `output.text` 作为
  transcript 的完整文本，`usage.duration`（秒）换算成 `duration_ms`。
- qwen3-asr-flash 保持原来的非流式 `output.choices[0].message.content` 解析不变——它本来就没有
  时间戳，字幕编辑器里显示不出分词是这个模型的固有限制，不需要（也没办法）用代码修。
- 新增单元测试 `parses_fun_asr_flash_sse_sample_from_docs`，直接用文档里给出的原始 SSE 示例
  文本做断言，锁定解析逻辑与文档描述一致。

## 补充：真实返回里 `sentence_id` 也可能漂移

后续联调又碰到一类更脏的返回：相邻两个 `sentence_end=true` 事件其实还是同一句在“继续定稿”，
但服务端给出的 `sentence_id` 已经变了。它们通常有这些共同点：

- `begin_time` 相同；
- `channel_id` / `speaker_id` 不变；
- 新事件的 `end_time` 更晚；
- 新事件的 `text` 是旧事件文本的扩展版（或完全相同）。

如果只按 `sentence_id` 去重，前端仍会出现一串 300ms 左右、文本却整句重复的字幕块。

因此现在的兜底规则变成两层：

1. `sentence_id` 相同：直接覆盖上一条；
2. 即使 `sentence_id` 变化，只要它与上一条满足“同起点 + 同说话人/声道 + 结束时间向后增长 + 文本前缀延长”，
   仍视为同一句的重复定稿，覆盖上一条而不是追加。

## 补充二：过期回声的 begin_time 会顺移到上一句的 end_time（不是同起点）

2026-07 又观察到第三种变体：上一句已经正常定稿（begin/end 是合理的整句时长），紧接着立刻
重发一遍**一模一样的整句文本**，但这次的 `begin_time` 不再和上一句相同，而是**等于上一句的
`end_time`**，`end_time` 只比 `begin_time` 晚 300ms 左右——明显不可能是新说出来的内容。
这种情况规则 2（要求 `begin_time` 相同）判断不出来，会被当成新句子追加，产生一条紧跟在正常
字幕后面、时长极短但文本完全重复的字幕块。

修复：新增第三条规则 `next_sentence_is_stale_echo`——`next.begin_time == last.end_time`
且文本与上一条重复（相等或互为前缀）、说话人相同时，判定为“过期回声”，**只把
`last.end_time` 延伸到 `next.end_time`，不覆盖 `last` 的其它字段**（因为这条回声的
begin_time/words 都不可信，真正准确的时间戳和文本在上一条里）。

对应实现：`src-tauri/src/providers/alibabacloud/transcription.rs` 中
`last_sentence_still_finalizing`（原规则 1+2，命中则整体覆盖上一条）与
`next_sentence_is_stale_echo`（新规则 3，命中则只延伸 end_time）两个函数，
测试见 `dedups_stale_echo_sentence_whose_begin_time_shifts_to_previous_end_time`。

排查这类问题时不要先假设是最近的重构（如模型注册表改造）引入的回归——先确认
`transcription_model_family`/`file_transcription_route` 是否仍把目标模型路由到预期分支，
再去看去重规则本身有没有覆盖到新的边界情况；这次的重复实际上是去重规则本身遗漏的一种
新变体，与注册表重构无关。

## 适用场景

以后如果同一个"多模型共用一个 HTTP 端点"的场景（同一个 URL、不同 `model` 字段值），
**不要假设它们返回体结构相同**——哪怕请求体结构相似，也要逐个模型对照文档核实响应结构，
必要时按 `family`/`model` 分别写解析函数，而不是写一个通用解析器硬套所有模型。
