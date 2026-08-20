# LLM 问答与普通处理的场景默认及输出边界

## 适用场景

智能问答与翻译、编辑、智能优化共用 LLM 配置，但请求参数和输出协议不同，不能只靠页面状态区分。

## 规则

- 智能问答默认使用较高推理强度、请求联网搜索，并走流式输出；正文按 Markdown 渲染，思考片段单独展示。
- 翻译、编辑和智能优化默认关闭推理与联网搜索，继续使用结构化 JSON，避免模型思考内容破坏领域解析或注入链路。
- 联网搜索只在当前 genai 版本能稳定编码为 OpenAI Responses `web_search` 的火山方舟、DeepSeek、阿里云百炼路由启用；Kimi、GLM、MiMo、MiniMax 的供应商专属搜索协议不能伪装成通用工具。
- DeepSeek V4 关闭思考必须发送 `thinking.type=disabled`，不能把 `ReasoningEffort::Zero` 编码成 `reasoning_effort=none`。
- Kimi K3 与 GLM-5.3 没有通用的“关闭思考”协议值，普通场景只能保留供应商默认行为，不能发送无效参数。

## 维护提示

新增供应商时，先确认它支持的协议、搜索工具编码和思考开关，再决定是否加入 `supports_web_search`、场景默认值或 Responses 路由；不要把所有 OpenAI 兼容接口都当成同一种能力。
