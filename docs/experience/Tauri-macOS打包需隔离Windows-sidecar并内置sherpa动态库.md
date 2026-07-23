# Tauri macOS 打包需隔离 Windows sidecar 并内置 sherpa 动态库

## 现象

- 公共 `tauri.conf.json` 声明 Windows 专用 `externalBin` 后，macOS 构建会查找不存在的 `*-aarch64-apple-darwin` sidecar，并在 Rust 编译前失败。
- macOS 即使成功生成 `.app`，主程序仍可能依赖 `@rpath/libsherpa-onnx-c-api.dylib` 和 `@rpath/libonnxruntime.*.dylib`；如果包内没有这些文件，应用离开 `target/release` 后无法启动。

## 正确做法

1. Windows 专用 sidecar 只放在 `tauri.windows.conf.json`，不要放在公共配置。
2. 在 `tauri.macos.conf.json` 的 `bundle.macOS.frameworks` 中声明 sherpa-onnx 与 ONNX Runtime 动态库。Tauri 会复制到 `Contents/Frameworks`，并为主程序补充 `@executable_path/../Frameworks` 的 `LC_RPATH`。
3. 打包后不能只看 `.app`/DMG 是否生成；还要用 `otool -L`、`otool -l` 确认依赖与 RPATH，并检查 `Contents/Frameworks` 的实际内容。
4. `ogg-opus` 在 macOS 构建时需要可链接的静态 Opus。项目的 npm Tauri 启动脚本会自动识别 Homebrew 在 Apple Silicon 与 Intel 上的常见安装路径；其他布局需显式设置 `OPUS_LIB_DIR`。

## 验证

- `npm run tauri:build -- --bundles app`
- `otool -L "说吧！.app/Contents/MacOS/SayIt"`
- `otool -l "说吧！.app/Contents/MacOS/SayIt" | grep -A2 LC_RPATH`
- `find "说吧！.app/Contents/Frameworks" -maxdepth 1 -type f`
