# 从 PowerShell 7 运行 Rust 构建时 ocr-rs 解压 MNN 失败

## 现象

在 PowerShell 7（pwsh）里运行 `npm run test:rust`、`npm run tauri:dev` 或 `cargo` 时，`ocr-rs` 的 build script 报错：

```
Prebuilt MNN extraction failed: lib/ not found in ...\target\debug\build\ocr-rs-*\out\prebuilt\mnn-dev-windows-x86_64
```

`out\prebuilt\` 下只有 `mnn-dev-windows-x86_64.zip`，没有解压出来的目录；删掉 `prebuilt\` 重建也一样。

## 原因

`vendor/ocr-rs/build.rs` 的 `extract_zip` 调用 Windows PowerShell 5.1（`powershell -Command Expand-Archive ...`）解压。从 pwsh 7 启动时，子进程继承 pwsh 的 `PSModulePath`，5.1 加载不了 `Microsoft.PowerShell.Archive` 模块，`Expand-Archive` 没有执行，但进程仍以成功退出，于是 build script 在后面检查 `lib/` 时才报错。

同一原因也会让 pwsh 里的 `Get-AppxPackage` 报「Operation is not supported on this platform」。

## 做法

- Rust 构建、测试和 `tauri:dev` 从 Git Bash 或 cmd 运行，不从 pwsh 运行。
- 已经失败过的构建目录不用手动清理，换到 Git Bash 重跑即可重新解压。
