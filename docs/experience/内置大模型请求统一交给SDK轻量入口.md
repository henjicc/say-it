# 内置大模型请求统一交给 SDK 轻量入口

## 背景

智能处理、助手、翻译、学习等的大模型请求曾由 Rust 用 genai 直连，思考参数、接口地址、Responses 路由都在 `smart_text.rs` 里手写，和 SDK 的写法逐渐不一致（例如 MiniMax M3、GLM-5.3 选「关闭」仍在思考）。现在全部交给 `@henjicc/ai-sdk` 的 `runLlmChatStream`，在 QuickJS 里执行（`sdk-runtime/llm-entry.ts`）。

## 要点

- **只能用轻量入口。** SDK 的模型步 `runModelStep` 依赖 Vercel AI SDK 和 Web Streams，QuickJS 里一加载就报 `TransformStream is not defined`，补上后每次建运行时还要多约 55ms。`runLlmChatStream` 不依赖这些，打包后约 80KB。
- **能力由 SDK 补全的条件。** 供应商有预设、模型在 SDK 目录里，SDK 才补能力；否则（手动添加、模型列表刷新得到的、自定义地址）不补，思考字段一个都不发。`llm-entry.ts` 用 SDK 自己的 `findLlmProviderPreset` + `findLlmModelCatalogEntry` 判断，不认识时才用宿主给的 `fallbackCapabilities`。
- **不请求 JSON 输出模式。** 目录里多数模型没声明 `structuredOutputMode` 或「思考时可用 JSON」，SDK 会直接拒绝请求。
- **QuickJS 白名单。** 构建脚本按文件路径导入 SDK 的 `protocolEndpoints.js`，算出每家默认配置下 Chat 与 Responses 的实际主机（`llmHosts`）；自定义地址只放行它自己的主机，明文 http 只放行本机。

## 以后

发现某家请求不对（字段、地址、协议），去 SDK 修，不在说吧加供应商分支。
