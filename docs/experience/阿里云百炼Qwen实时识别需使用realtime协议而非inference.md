# 阿里云百炼 Qwen 实时识别需使用 realtime 协议而非 inference

## 触发条件

- 需要在同一个桌面端实时识别链路里同时支持 `fun-asr-realtime`、`paraformer-realtime-*` 和 `qwen3-asr-flash-realtime`。

## 正确做法

- `Fun-ASR` / `Paraformer` 继续走 `wss://dashscope.aliyuncs.com/api-ws/v1/inference`，通过 `run-task` / `finish-task` 和二进制音频帧交互。
- `Qwen3-ASR-Flash-Realtime` 必须改走 `wss://dashscope.aliyuncs.com/api-ws/v1/realtime?model=...`，并额外带上请求头 `OpenAI-Beta: realtime=v1`。
- `Qwen` 的会话初始化要先发 `session.update`，音频通过 `input_audio_buffer.append` 以 base64 文本事件发送，结束时发 `session.finish`。
- 结果解析也要分流：
  - `Fun/Para` 看 `header.event=result-generated` 和 `payload.output.sentence`
  - `Qwen` 看 `conversation.item.input_audio_transcription.text` / `completed`

## 备注

- 这两套协议不能只靠切模型名复用同一套消息格式，否则会出现连接成功但服务端不返回有效识别结果的问题。
