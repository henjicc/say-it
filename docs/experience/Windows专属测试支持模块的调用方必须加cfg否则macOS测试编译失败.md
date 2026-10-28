# Windows 专属测试支持模块的调用方必须加 cfg，否则 macOS 测试编译失败

## 现象

macOS CI 的 `npm run test:rust` 编译期报错，Windows 正常：

```
error[E0432]: unresolved import `crate::performance_test_support`
note: found an item that was configured out ... #[cfg(all(test, windows))]
```

## 根因

`performance_test_support.rs` 直接链接 `kernel32`（`K32GetProcessMemoryInfo`、`QueryThreadCycleTime`），在 `main.rs` 中以 `#[cfg(all(test, windows))]` 声明。跨平台模块里的性能测试如果引用它却漏写 `#[cfg(windows)]`，即使测试带 `#[ignore]` 也会参与编译，在 macOS 上整个测试二进制编译失败，所有 Rust 测试都不会运行。

## 做法

- 在跨平台模块中新增引用 `crate::performance_test_support` 或其他 Windows 专属模块的测试时，与同目录已有的性能用例保持一致，在 `#[test]` 前加 `#[cfg(windows)]`。
- 本机只有 Windows 时无法发现这类问题；推送后必须看 macOS CI job 的结果，不能只看 Windows job 通过。CI 持续失败时先读 `gh run view <id> --log-failed`，不要把失败当成常态。
