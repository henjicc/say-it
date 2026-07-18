# PP-OCR 内存占用实测与 MNN 本地编译结论

## 背景

用户观察到启用 PP-OCR 后任务管理器内存上涨 300-400 MB，怀疑引擎常驻内存过高，并提出"本地编译 CPU 专用 MNN 是否更优"。通过在 `src-tauri/vendor/ocr-rs`（实际编译的定制版，`docs/others/rust-paddle-ocr` 是未定制上游）写临时探针测试得到硬数据。

## 实测数据（PP-OCRv6 tiny，3 线程，Collect 模式，det 960，与应用配置一致）

| 场景 | det/rec 会话内存 | 进程稳态 | 峰值 | drop 引擎后 |
|---|---|---|---|---|
| 轻负载（2 文字块） | 66 / 10 MiB | 93 MB | ~110 MB | 13 MB |
| 密集文本 + 多窗口尺寸轮换 | 83 / 28 MiB | 128 MB | 168 MB | 8 MB |
| 最坏情况（76 块打满） | 83 / 30 MiB | 131 MB | 173 MB | 7 MB |

关键事实：

- **引擎构建（含 SHA-256 校验 + MNN 建会话）只要约 3 ms**（模型仅 3 MB）；1600px 整窗识别 70-400 ms。"常驻引擎避免冷启动"的设计前提不成立。
- **drop 引擎后内存完整归还系统**（回落到 <15 MB），无泄漏无碎片滞留。
- 识别会话内存随文本密度增长（10→30 MiB），检测会话随窗口比例变化增长（66→83 MiB）。
- **任务管理器看到的 300-400 MB 是应用进程组合计**（说吧.exe + msedgewebview2.exe 若干）；Rust 侧上界 ~170 MB，其余是调试窗口 WebView 渲染 base64 大截图的开销。分析内存问题时用「详细信息」页签分进程看。
- `BackendMemoryMode::Low` 与 `PrecisionMode::Low` 在本机（Ryzen 9 5900X）完全无效果，不必采用。

**由此的架构决策**：PP-OCR 引擎改为按任务构建、任务结束即释放（`ocr.rs` worker 内），删除了原有的 `RELEASE_REQUESTED`/设置切换释放机制。

## MNN 本地编译研究结论（不做）

- 预编译包（`vendor/mnn-dev-windows-x86_64.zip`，来自 zibo-chen/MNN-Prebuilds，MNN 3.4.1）**已内置 SSE + AVX2 + FMA 内核并在运行时按 CPUID 调度**——用 `strings MNN.lib` 可见 `GemmSSE/GemmAVX2/GemmAVX2FMA/AVX2Functions` 对象，无 AVX512 对象（`MNN_AVX512` 默认 OFF）。
- Ryzen 9 5900X（Zen 3）**不支持 AVX-512**，本地编译开 `MNN_AVX512` 在此硬件上零收益；AVX2+FMA 路径已经在跑。
- ocr-rs 的 `build-mnn-from-source` 特性会 clone MNN 3.4.1 + CMake（`MNN_PORTABLE_BUILD=ON`，NMake），只会给开发与 CI 增加 cmake/NMake/git 负担，热点算子是手写 intrinsics，编译器标志的收益仅个位数百分比。
- 结论：**"本地编译 CPU 专用推理引擎更优"在本硬件上不成立，维持预编译包。** 若未来目标用户普遍是支持 AVX-512 的 CPU，才值得重新评估。

## 探针方法备忘

vendored crate 是 lib crate 但 `autotests = false`：临时在其 `Cargo.toml` 加 `[[test]]` 条目 + `tests/xxx.rs`，用 `cargo test --test xxx --no-default-features --features mnn-static -- --nocapture` 运行，测完删除两处改动。测 RSS 用 `K32GetProcessMemoryInfo`（working_set + pagefile_usage 及各自峰值，对应任务管理器的「内存」与「提交大小」列）。负载必须够密：轻负载 fixture 会严重低估 rec 会话内存。
