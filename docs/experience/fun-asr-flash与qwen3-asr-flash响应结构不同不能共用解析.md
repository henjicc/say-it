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

## 适用场景

以后如果同一个"多模型共用一个 HTTP 端点"的场景（同一个 URL、不同 `model` 字段值），
**不要假设它们返回体结构相同**——哪怕请求体结构相似，也要逐个模型对照文档核实响应结构，
必要时按 `family`/`model` 分别写解析函数，而不是写一个通用解析器硬套所有模型。
