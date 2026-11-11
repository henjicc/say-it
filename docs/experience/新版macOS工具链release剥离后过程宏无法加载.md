# 新版 macOS 工具链 release 剥离后过程宏无法加载

## 现象

在 macOS 27（ld-27037）上设置 `MACOSX_DEPLOYMENT_TARGET=15.5` 后做 release 构建（`tauri:build`、`llm-engine:build`），编译到依赖 `serde_derive` 等过程宏的 crate 时失败：

```
dlopen(.../release/deps/libserde_derive-*.dylib): mis-aligned LINKEDIT string pool
```

删掉该过程宏的产物重编也一样；debug 构建正常，不设部署版本变量的 release 构建也正常。

## 根因

Cargo 的 release profile 默认 `strip = "debuginfo"`。部署版本低于本机系统时，新工具链剥离后的 dylib 字符串表没有对齐，dyld 拒绝加载。过程宏和构建脚本是在编译期由 rustc 加载的宿主 dylib，所以构建直接中断；最终的可执行文件不经 dlopen，不受影响。

## 做法

`src-tauri/Cargo.toml` 设置 `[profile.release.build-override] strip = false`。这样只对过程宏、构建脚本及其依赖关闭剥离，它们不进安装包，产物大小与行为都不变。

不要为此去掉 `MACOSX_DEPLOYMENT_TARGET`：`scripts/macos-build-env.mjs` 设置它，是为了让 cc / CMake 编出的原生代码与应用声明的最低系统版本一致。

## 验证

单独 `cargo build --release -p serde_derive` 不能用来验证：此时过程宏是普通构建目标，不走 `build-override`。要用真实路径验证，即 `npm run llm-engine:build` 或 `npm run tauri:build`。
