# sherpa-onnx 与 MNN 在 Windows 不可同时静态链接

## 触发条件

Windows/MSVC 下，应用已经通过 `ocr-rs` 的 `mnn-static` 特性静态链接 MNN，又接入 `sherpa-onnx` 默认静态运行库。

## 现象

- `cargo check` 和单独的 sherpa-onnx 真实识别都能通过。
- 一旦初始化 PP-OCR/MNN，测试进程会以 `STATUS_ACCESS_VIOLATION (0xc0000005)` 退出；完整测试并发时通常表现为所有用例打印通过后进程异常退出。
- 这不是 Rust 会话生命周期问题：只运行不触发 MNN 的测试不会崩溃，单独运行 PP-OCR 夹具则能稳定复现。

## 正确做法

1. sherpa-onnx 使用 `default-features = false, features = ["shared"]`，隔离 ONNX Runtime 与现有 MNN 静态库；不要同时把两套原生推理运行时静态链接进主程序。
2. `sherpa-onnx-sys` 的 Windows 共享构建会把 DLL 复制到 `target/<profile>`，但 Cargo 测试程序位于 `target/<profile>/deps`。项目 `build.rs` 需要同步复制 4 个 DLL 到 `deps`，否则 Windows 会弹出不可见的缺 DLL 对话框，自动化表现为测试永久挂起。
3. Tauri NSIS 不会自动把动态依赖放到安装目录根部。把 `target/release` 下 4 个 DLL 加入 Windows bundle resources，再由 `NSIS_HOOK_POSTINSTALL` 复制到 `$INSTDIR`；卸载钩子同步删除。
4. 验证不能只看 `cargo check`：至少运行 PP-OCR 原生夹具、sherpa 真实模型识别、完整 `cargo test` 和一次 NSIS 构建，并检查最终安装包确实包含 4 个 DLL 资源。

涉及 DLL：`onnxruntime.dll`、`onnxruntime_providers_shared.dll`、`sherpa-onnx-c-api.dll`、`sherpa-onnx-cxx-api.dll`。
