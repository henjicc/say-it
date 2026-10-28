# 本机 Rust 构建失败：NASM 不支持中文路径

## 现象

`npm run test:rust`、`npm run tauri:dev` 或 `cargo build --release` 在 `aws-lc-sys` 构建脚本 panic：
`NASM failed for ...chacha-x86_64.asm`，错误输出为空。debug 与 release 都会出现。

## 根因

`aws-lc-sys` 只要在 PATH 上找到 `nasm` 就用它汇编，找不到才改用随包附带的预编译对象
（本项目经 `aws-lc-rs` 已启用 `prebuilt-nasm` 特性）。本机 PATH 上的 `nasm` 来自 Strawberry Perl
（`C:\Strawberry\c\bin\nasm.exe`，2.16.01），在 Windows 上走 ANSI 文件 API，
**输出路径含非 ASCII 字符时无法创建文件**。本仓库路径 `D:\VibeCode\说吧\` 含中文，
`OUT_DIR` 在 `src-tauri/target/` 下，所以 NASM 写不出 .obj。

debug 构建看似不受影响，只是因为 `aws-lc-sys` 的旧构建产物还在缓存里；工具链升级、
依赖变更或清理缓存后它必须重新构建，debug 同样失败。

手工验证：

```bash
nasm -f win64 <asm> -o "D:/VibeCode/说吧/src-tauri/target/release/x.obj"   # fatal: unable to open output file
nasm -f win64 <asm> -o /tmp/x.obj                                          # 路径正常（报缺 include 是另一回事）
```

## 解决

- 推荐：让构建找不到这份 NASM（从 PATH 移除 `C:\Strawberry\c\bin`，或给其中的 `nasm.exe` 改名），
  `aws-lc-sys` 会改用预编译对象，不再依赖本机 NASM。改完后重开终端和编辑器使 PATH 生效。
- 临时：`CARGO_TARGET_DIR` 指到纯 ASCII 路径只适合跑测试或单独 `cargo build`。
  `tauri.windows.conf.json` 的打包资源写死了 `target/release/*.dll`，换了 target 目录后
  打包会找不到这些 DLL，或误用 `src-tauri/target/release` 里的旧文件。
- `AWS_LC_SYS_NO_ASM=1` **只对 debug 构建有效**，release 下构建脚本会直接
  panic（"AWS_LC_SYS_NO_ASM only allowed for debug builds"）。
- 正式 release 由 GitHub Actions 构建（英文路径），不受影响。
