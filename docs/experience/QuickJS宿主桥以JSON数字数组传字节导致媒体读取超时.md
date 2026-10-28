# QuickJS 宿主桥以 JSON 数字数组传字节导致媒体读取超时

## 现象

`providers::sdk_runtime::tests::quickjs_multipart_uses_native_request_body_without_json_array_oom` 在 Windows CI 上几乎每次报「插件操作超时」，偶尔通过；macOS CI 与本机通过。本机调试构建单独跑也要 9.76 秒，紧贴 10 秒调用预算，CI 上与 700 多个测试并发时就超时。

## 根因

计时后发现 9.07 秒全耗在 `runtime.media.read`，真正的 multipart 请求只要 38ms。`media.read` 按 64 KiB 分块调用 `__sayitHostCall('media.readChunk')`，宿主把每块 `Vec<u8>` 包成 `json!({"bytes": bytes})`：Rust 侧给每个字节建一个 `Value` 再序列化成文本，JS 侧 `JSON.parse` 成 65536 元素的普通数组再逐元素 `set` 进 Uint8Array。10 MiB 就是 160 次这样的往返，调试构建下 Rust 与 QuickJS 都未优化，耗时以秒计。这条路径在 9 月 24 日之前就存在，当时已有偶发失败，不是某次提交引入的回归；单测本身就几乎用满预算，CI 并发负载稍高就超时。

## 做法

- QuickJS 与宿主之间传二进制，一律走原生函数直接收发 `TypedArray<u8>`（如 `__sayitHostStoreRequestBody`、`__sayitHostReadMediaChunk`），不要把字节放进 JSON 数组。
- 新增宿主全局函数时，所有加载 host-adapter 的 QuickJS 上下文都要注册：除 `plugin_runtime.rs` 外，`plugin_capability.rs` 的两处清单校验上下文也会初始化 host-adapter，缺少函数会在初始化时直接抛错。
- 测试超时先计时定位耗时分布，再判断是测试等待方式还是产品路径的问题；不要直接加大截止时间。
