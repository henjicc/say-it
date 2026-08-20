# Groq Qwen 思考内容破坏结构化 JSON 输出

## 触发条件

通过 Groq 调用支持推理的 Qwen 3 系列模型，并要求正文返回最终文本或 JSON。即使提示词明确要求“只返回结果”，模型在默认推理模式下仍可能把 `<think>...</think>` 放进 `message.content`，导致智能优化直接注入思考过程，或让结构化解析失败；输出也会变长并增加延迟。若生成在 `</think>` 前达到 Token 上限，响应里甚至完全没有最终正文。

## 正确做法

- 对需要机器解析的助手请求启用 OpenAI 兼容协议的 JSON Object Mode（`response_format.type = json_object`）。
- Groq Qwen 3.6 27B 的 `reasoning_effort` 只接受 `none` 和 `default`：关闭思考时发送 `none`，开启思考时发送 `default`。不能把应用内的 `low`、`medium` 或 `high` 原样发送，否则 Groq 会返回 400。
- 上层仍可使用统一的推理强度配置，但必须在 Groq Qwen 模型边界做二值映射：`zero -> none`，`low/medium/high -> default`。智能问答开启思考时同时发送 `reasoning_format = parsed`，让流式接口将思考与正文分字段返回。
- 不要把这一参数无条件发送给其他推理模型。例如 Groq 的 GPT-OSS 使用 `low`、`medium`、`high`，不支持 `none`。
- Rust 在统一模型边界剥离完整的 `<think>...</think>` 块，并校验最终正文非空。遇到未闭合标签必须判为失败，让听写领域保留原文供用户恢复，禁止把半截思考过程注入目标软件。
- Rust 仍需校验 JSON、意图枚举和非空正文，不能把供应商的结构化输出保证当作领域校验的替代品。
- 解析器可以兼容代码围栏或 JSON 前后的少量文字，但这只能用于失败恢复，不能代替请求端的 JSON 模式。

## 验证

真实请求使用 `qwen/qwen3.6-27b`：默认模式返回了大段 `<think>` 正文；加入 `reasoning_effort = none` 与 JSON Object Mode 后，返回内容可直接解析为 `{"intent":"email","text":"..."}`。

Groq 官方说明：Qwen 3.6 27B 可用 `reasoning_effort = none` 关闭推理、用 `default` 开启推理；结构化场景可使用 JSON Object Mode，推理展示也可通过 `reasoning_format` 控制。
