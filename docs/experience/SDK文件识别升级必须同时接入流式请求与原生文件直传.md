# SDK 文件识别升级必须同时接入流式请求与原生文件直传

从 `@henjicc/ai-sdk` 0.2.x 升到 0.5.0 及以上时，仅修改依赖版本不能解决长音频限制。旧宿主的 `media.read()` 会把文件放进 QuickJS 的 64 MiB 堆，10 MiB 保护不能放宽。

- 百炼异步本地文件必须实现 `transport.uploadFile()`：从本次请求授权的媒体句柄打开文件，在联网前检查 SDK 提供的大小上限，由 Rust 生成 multipart，文件部分放最后。不得经由 JS 读取或重组整个文件。
- JSON/Base64 和普通 multipart 路径实现 `media.describe/readChunk` 与 `transport.fetchStream`，用有界队列传递分块并校验声明长度，不自动重放请求或跟随上传重定向。
- 网络背压和等待响应期间必须让出 QuickJS 作业循环，否则 SDK 的 AbortSignal 和计时器不能运行；同时保留 Rust 取消令牌与总时限兜底。异常、取消和销毁均关闭上传任务、文件与响应流。
- QuickJS 的纯 JS UTF-8 编码会显著拖慢 Base64 请求。通用 `TextEncoder` 使用宿主二进制返回值，避免再经 JSON 数组传输。不要在宿主复制 SDK 的供应商解析器或 Base64 请求构造逻辑。

回归入口：`npm run test:rust -- providers::sdk_runtime`。本地夹具覆盖 80 MiB 原生上传、13 MiB 音频生成超过 16 MiB 的分块请求、百炼凭证/上传/提交/轮询/结果链路、取消、超限、文件变化和重定向不重放；不需要真实凭据或付费请求。
