# Groq Qwen 思考内容破坏结构化 JSON 输出

## 触发条件

通过 Groq 调用支持推理的 Qwen 3 系列模型，并要求正文返回 JSON。即使提示词明确要求“只返回 JSON”，模型在默认推理模式下仍可能把 `<think>...</think>` 放进 `message.content`，导致解析失败、输出很长且延迟增加。

## 正确做法

- 对需要机器解析的助手请求启用 OpenAI 兼容协议的 JSON Object Mode（`response_format.type = json_object`）。
- 仅对 Groq 明确支持非思考模式的 Qwen 3 系列发送 `reasoning_effort = none`；在 `genai` 中对应 `ReasoningEffort::Zero`。
- 不要把这一参数无条件发送给其他推理模型。例如 Groq 的 GPT-OSS 使用 `low`、`medium`、`high`，不支持 `none`。
- Rust 仍需校验 JSON、意图枚举和非空正文，不能把供应商的结构化输出保证当作领域校验的替代品。
- 解析器可以兼容代码围栏或 JSON 前后的少量文字，但这只能用于失败恢复，不能代替请求端的 JSON 模式。

## 验证

真实请求使用 `qwen/qwen3.6-27b`：默认模式返回了大段 `<think>` 正文；加入 `reasoning_effort = none` 与 JSON Object Mode 后，返回内容可直接解析为 `{"intent":"email","text":"..."}`。

Groq 官方说明：Qwen 3 可用 `reasoning_effort = none` 关闭推理；结构化场景可使用 JSON Object Mode，推理展示也可通过 `reasoning_format` 控制。
