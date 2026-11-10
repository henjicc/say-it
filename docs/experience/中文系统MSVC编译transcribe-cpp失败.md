# 中文系统 MSVC 编译 transcribe.cpp 失败

## 现象

`transcribe-cpp-sys` 的构建脚本在 `src\arch\canary\model.cpp` 报一串语法错误（`C2146 缺少";"`、`C4430 缺少类型说明符`、`C2668 对重载函数的调用不明确`），而同一份源码在英文系统和 CI 上能编过。

## 根因

本机系统代码页是 936（GBK）。MSVC 对没有 BOM 的源文件按系统代码页解码，transcribe.cpp 自己的源文件是 UTF-8 且含非 ASCII 字符，注释里的多字节字符会把行尾换行吞掉，后面的代码就错位了。它内置的 ggml 自带 `/utf-8` 编译选项，所以只有 transcribe.cpp 本体出错。

## 解决

仓库根的 `.cargo/config.toml` 给 MSVC 目标加 `CXXFLAGS_x86_64_pc_windows_msvc = "/utf-8"`。

- 只设 C++，不要设 `CFLAGS`：`aws-lc-sys` 一见到 `CFLAGS` 就改走另一条构建路径，探测结果出错，链接时报 `无法解析的外部符号 __builtin_bswap32`。
- 配置必须放仓库根：`cargo --manifest-path src-tauri/Cargo.toml` 按当前目录向上找配置，放在 `src-tauri/.cargo` 里不生效。
- 用带目标后缀的变量名，macOS 构建不受影响。

## 同时要有 Vulkan SDK

Windows 的 `vulkan` 特性在构建时用 SDK 里的 `glslc` 编译着色器，需要环境变量 `VULKAN_SDK`。官方安装器加 `--root <目录> --accept-licenses --default-answer --confirm-command install copy_only=1` 可以只复制文件、不写注册表；这种装法不会设环境变量，要自己设。
