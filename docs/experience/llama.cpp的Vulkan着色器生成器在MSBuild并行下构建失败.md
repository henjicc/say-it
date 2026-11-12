# llama.cpp 的 Vulkan 着色器生成器在 MSBuild 并行下构建失败

## 现象

`npm run llm-engine:build`（`tauri:dev`、`tauri:build` 都会先跑它）在 `llama-cpp-sys-2` 的构建脚本里失败，报下面任一错误：

- `MSB1009 项目文件不存在`、`could not load cache`；
- `file INSTALL cannot find ".../vulkan-shaders-gen-build/Release/vulkan-shaders-gen.exe": File exists.`（路径里的「说吧」显示成乱码，但不是编码问题）。

## 根因

`vulkan-shaders-gen` 是 llama.cpp 内嵌的 ExternalProject，MSBuild 并行时它的配置、编译、安装步骤会互相抢跑。已完成的步骤会留在 `out/build` 里，重跑通常就能过。

## 什么时候会碰到

`llama-cpp-sys-2` 换了新的构建目录时才重编：改了 `[profile.release.build-override]`，或者构建脚本声明的环境变量（例如 `PATH`）变了。`tauri:dev` 与 `tauri:build` 的环境不同，会各自重编一次。

## 做法

`scripts/build-llm-engine.mjs` 在 Windows 上失败后会原样重试，最多 3 次。仍然失败时手动再跑一次 `npm run llm-engine:build`，不要删掉 `target/release/build/llama-cpp-sys-2-*`，那样会从头重编。
