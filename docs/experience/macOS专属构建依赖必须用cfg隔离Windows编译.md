# macOS 专属构建依赖必须用 cfg 隔离 Windows 编译

## 触发条件

`Cargo.toml` 仅在 macOS 的 `build-dependencies` 中声明 `cc`，但 `build.rs` 的普通函数仍引用 `cc::Build`，会使 Windows 构建报 `E0433: unresolved module or unlinked crate cc`。

## 根因与处理

函数内检查 `CARGO_CFG_TARGET_OS` 并提前返回属于运行期判断，不能阻止 Rust 编译器解析函数体中的依赖引用。对仅在 macOS 构建主机可用的原生桥构建函数及调用点，使用 `#[cfg(target_os = "macos")]` 做编译期隔离；保留原有目标平台检查，避免 macOS 主机构建其他目标时编译 Objective-C 桥。

不要只为消除 Windows 错误而把 macOS 专用依赖扩大到所有平台。验证时应实际执行 Windows 构建，不能仅确认 macOS 分支运行时不会被调用。
