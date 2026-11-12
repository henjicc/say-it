# DeepSeek 走 Responses 接口时关闭思考无效

## 现象

DeepSeek Flash 的推理强度设为「关闭」，智能处理一段 777 字的口述仍要 10.8 秒；开发者 `POST /smart-test` 里 `reasoningEffective` 显示 `zero`，`usage.reasoningTokens` 却有 1646。

## 根因

DeepSeek Flash 在 `SDK_LLM_RESPONSES_MODELS` 里，走 Responses 接口（日志里模型名带 `openai_resp::` 前缀）。`chat_options` 先匹配了 DeepSeek 分支，发的是 Chat Completions 的写法 `thinking: {type: "disabled"}`；Responses 接口不认这个字段，按默认的高强度思考执行。DeepSeek 文档里 Responses 关闭思考要用 `reasoning: {effort: "none"}`，`minimal` 也会被映射成 `low`，同样不算关闭。

## 做法

`application/smart_text.rs` 的 `chat_options` 里，「关闭」先按是否走 Responses 判断，走 Responses 一律发 `reasoning.effort = "none"`；只有走 Chat 接口时才用各家的 Chat 写法。修好后同一段 2.5 秒、无思考 token，短句约 1.1 秒。

## 排查

怀疑思考没关时，用 `POST /smart-test`（带 `providerId`、`model`、`text`）看 `llm[].usage.reasoningTokens`；不要用 `POST /model-bench` 的 `fromRun` 重放本地整理模型的那次听写，那次没有云端模板，重放的只是原文。
