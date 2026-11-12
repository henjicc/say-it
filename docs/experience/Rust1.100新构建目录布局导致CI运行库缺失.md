# Rust 1.100 新构建目录布局导致 CI 运行库缺失

## 现象

- CI 的 `npm run test:rust` 在两个平台同时开始失败，代码没有相关改动；本机（旧版 Rust）一切正常。
- macOS：`build.rs` panic「缺少运行库 …/target/debug/libsherpa-onnx-c-api.dylib，无法准备打包」。
- Windows：编译成功，测试程序启动即失败，`exit code: 0xc0000135, STATUS_DLL_NOT_FOUND`。

## 根因

- `dtolnay/rust-toolchain@stable` 拉到 Rust 1.100，Cargo 改用新的构建目录布局：每个包在 `target/<profile>/build/<包名>/<哈希>/` 下分 `fingerprint/`、`out/`、`run/`，测试程序也在 `out/` 里，不再有 `deps/` 和 `.fingerprint/`。
- macOS：sherpa-onnx-sys 的构建脚本把运行库复制到 `target/<profile>/`，这是 Cargo 不跟踪的副作用。rust-cache 恢复了它的指纹，却没保存复制出来的库，Cargo 不会重跑它；`run-rust-tests.mjs` 原有的缓存修复只认旧布局目录名，且只在预编译包缺失时触发。
- Windows：`build.rs` 把 DLL 复制到 `deps/` 供测试程序加载，新布局下测试程序不在那里。

## 做法

- `scripts/run-rust-tests.mjs`：当前 profile 缺运行库也视为缓存不完整，删除 sherpa-onnx-sys 的构建记录（`build/` 下匹配 `^sherpa-onnx-sys(-|$)`，兼容新旧布局）让它重跑。
- Windows 先 `cargo test --no-run --message-format=json-render-diagnostics`，按 Cargo 报告的 `executable` 把 `target/runtime-libs/` 复制到应用包测试程序旁边，再正式运行。不能改用 PATH：System32 里有旧版 `onnxruntime.dll`，搜索顺序先于 PATH。
- 本机复现：`rustup toolchain install <版本> --profile minimal`，用 `RUSTUP_TOOLCHAIN=<版本> CARGO_TARGET_DIR=<src-tauri/target 下的子目录>` 跑 `node scripts/run-rust-tests.mjs`；删掉 profile 目录下的运行库和 `build/say-it` 即可模拟「缓存只恢复依赖」的状态。用完删掉这个子目录（一次完整构建约 3 GB）。
- CI 突然在两个平台同时失败、代码又无关时，先对比日志里 `rust-toolchain` 步骤打印的 rustc 版本。
