# QuickJS 宿主同步 block_on 叠加 Tokio 栈导致栈溢出

## 触发条件

当 QuickJS 插件或内置 SDK 在 JavaScript 调用栈中执行 `__sayitHostCall`，宿主又在同一线程使用 `block_on` 驱动 reqwest/hyper/rustls 时，解释器栈与完整网络栈会叠加。该线程如果来自 Tokio blocking 池，崩溃只会显示 `tokio-rt-worker has overflowed its stack`，全局放大 worker 栈只能降低频率。

## 正确边界

- 所有 QuickJS 会话、一次性能力、LLM、翻译和模型发现统一运行在 `sayit-js-*` 专属命名线程，不占用 Tokio blocking 池。
- QuickJS 保留 1 MiB 解释器上限；专属线程为解释器、序列化和窄宿主桥提供 4 MiB 栈。该参数只属于 JS 边界，不能传播到 Tokio 全局线程。
- `http.request`、`http.stream.open` 和 `http.stream.read` 把 Future 投递到 Tokio 异步运行时，再通过一次性通道把结果交还同步 QuickJS。禁止在 QuickJS 宿主回调内恢复 `block_on`。
- 长时间实时会话不能把生命周期当作初始化超时。运行时初始化限制为 30 秒，后续 send/finish 各自刷新宿主截止时间；SDK `openSession` 不接收整段会话的固定 `timeoutMs`。

## 取证约束

Windows 启动时注册常驻 vectored exception handler。Tokio 与 QuickJS 线程预留 64 KiB 异常处理栈，并登记线程名。发生 `STATUS_STACK_OVERFLOW` 时，只使用固定缓冲区和预打开句柄写入 `say-it-stack-overflow.log`，记录线程、模块基址、异常地址和帧地址；诊断包会携带该文件。不要在异常处理器中分配内存、格式化字符串或走常规异步日志。

## 回归检查

- `plugin_runtime.rs` 及其所有产品调用方中不得出现 `block_on` 或 QuickJS `spawn_blocking`。
- 测试应证明宿主 I/O 与调用线程不同，且 QuickJS 使用 `sayit-js-*` 专属线程。
- capability 校验运行时必须同时注册 `__sayitHostCall` 与 `__sayitHostStoreRequestBody`，否则加载当前 SDK bootstrap 会在校验前直接失败。
