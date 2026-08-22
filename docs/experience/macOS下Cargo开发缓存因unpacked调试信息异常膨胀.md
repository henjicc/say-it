# macOS 下 Cargo 开发缓存因 unpacked 调试信息异常膨胀

## 现象

`src-tauri/target/debug` 在频繁开发后达到数十 GB，`deps/` 中出现大量
`*.rcgu.o`。目录的逻辑大小还可能因硬链接明显高于实际磁盘占用。

## 原因

Cargo 的 macOS 开发配置默认启用完整调试信息和 `unpacked` 拆分模式，
每个代码生成单元都会保留带调试信息的对象文件。大型 Rust 桌面项目在
依赖、特性或编译输入变化后会留下多代产物，长期不清理时会持续累积。

## 项目约定

根包的开发配置使用有限调试信息和 `packed` 模式：

```toml
[profile.dev]
debug = 1
split-debuginfo = "packed"
```

这样仍保留开发态栈回溯和基本调试能力，同时避免 macOS 为每个编译单元
散落并重复链接对象文件。若历史缓存已经膨胀，使用 Cargo 的 dev profile
清理功能删除后重新构建；不要手工选择性删除 `deps/` 或 `incremental/`，
以免留下不一致缓存。

## 验证

配置生效后执行一次完整开发构建，确认：

- 构建成功；
- `target/debug/deps` 不再生成 `*.rcgu.o`；
- 调试符号集中在 macOS 的 `.dSYM` 中；
- 新鲜开发构建的 `target/debug` 维持在项目依赖规模对应的数 GB，而不是数十 GB。
