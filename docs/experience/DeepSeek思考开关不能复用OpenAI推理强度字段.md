# DeepSeek 思考开关不能复用 OpenAI 推理强度字段

## 问题

智能处理通过 `genai` 统一调用大语言模型时，把 UI 的「推理强度：关闭」直接映射为
`ReasoningEffort::Zero`。这对 DeepSeek V4 会生成 `reasoning_effort: "none"`，接口立即拒绝请求，
听写流程随后终止且没有文本注入。

## 根因

`genai` 的 DeepSeek 适配器负责替换 Endpoint 和鉴权，实际请求仍委托给 OpenAI 兼容协议。
统一的 `ReasoningEffort` 只是调用提示，不保证每个供应商使用相同字段和值。

DeepSeek V4 关闭思考使用的是：

```json
{
  "thinking": {
    "type": "disabled"
  }
}
```

启用思考则使用 `thinking.type = "enabled"`；`reasoning_effort` 只用于控制已启用思考的强度。

## 正确做法

- 在应用层按供应商转换模型选项，不把统一类型直接视为供应商协议。
- DeepSeek 的 `zero` 只下发 `thinking.type = "disabled"`，不得同时生成
  `reasoning_effort = "none"`。
- DeepSeek 的显式非零推理强度下发 `thinking.type = "enabled"`，再交给 `genai` 编码
  `reasoning_effort`。
- `auto` 不附加专属字段，保留供应商默认行为。
- 供应商专属字段通过 `ChatOptions::with_extra_body` 注入，不绕过 `genai` 重写 HTTP 调用。
- 思考开启时使用更长超时；关闭思考仍沿用普通请求超时，避免失败时无谓等待。

对应实现和回归测试位于 `src-tauri/src/application/smart_text.rs`。
