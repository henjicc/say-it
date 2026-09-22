# 本机 release 构建失败：NASM 不支持中文路径

## 现象

`cargo build --release` 在 `aws-lc-sys` 构建脚本 panic：
`NASM failed for ...chacha-x86_64.asm`，错误输出为空。

## 根因

本机 `nasm`（Strawberry Perl 自带，2.16.01，版本本身满足要求）在 Windows 上走 ANSI
文件 API，**输出路径含非 ASCII 字符时无法创建文件**。本仓库路径 `D:\VibeCode\说吧\`
含中文，`target/release/` 在其下，所以 NASM 写不出 .obj。

手工验证：

```bash
nasm -f win64 <asm> -o "D:/VibeCode/说吧/src-tauri/target/release/x.obj"   # fatal: unable to open output file
nasm -f win64 <asm> -o /tmp/x.obj                                          # 路径正常（报缺 include 是另一回事）
```

debug 构建能过是因为 aws-lc-sys 在 debug 下走了不同的已缓存/预编译路径，没触发 NASM。

## 解决

- 正式途径：把 `CARGO_TARGET_DIR` 指到纯 ASCII 路径再构建，例如
  `CARGO_TARGET_DIR=C:\sayit-target cargo build --release`。
- 注意 `AWS_LC_SYS_NO_ASM=1` 这条路**只对 debug 构建有效**，release 下构建脚本会直接
  panic（"AWS_LC_SYS_NO_ASM only allowed for debug builds"）。
- 正式 release 由 GitHub Actions 构建（英文路径），不受影响。
