# SDK 新增识别预设前核对各协议是否真的发送热词

## 现象

升级 `@henjicc/ai-sdk` 到 0.10.0 时，照 Qwen-Audio 3.1 的目录条目复制出 3.0 三个预设，实时预设也标了 `supportsVocabulary: true`。界面显示支持热词，宿主也照常送出 `vocabulary`，但识别结果完全不受热词影响，也不报错。

## 原因

SDK 的文件预设给 3.0 声明了 `supports: { vocabulary: true }`，但实时协议 `dist/capabilities/speech-recognition/bailian/realtime/protocol.js` 只在 `preset.modelId === 'qwen-audio-3.1-asr-flash-streaming'` 时把 `vocabulary` 写进 `run-task`。同一协议族的其他模型收到的热词会被丢弃。

## 做法

- 新增或升级识别预设时，逐个协议到 SDK 的 `protocol.js` / `module.js` 里确认热词、上下文、方言等参数真的进了请求，不要只看预设的 `supports` 或照抄同族条目。
- SDK 不发送的能力，目录里标为不支持，并从 `sdk_runtime/online.rs::sdk_inline_vocabulary` 的模型列表里去掉；不得在本仓绕过 SDK 自行补参数。确需支持时回 Henji-AI 主仓修 SDK。
