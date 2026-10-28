# Cargo 开发缓存因调试信息拆分与旧代产物膨胀

## 现象

`src-tauri/target/debug` 在频繁开发后达到数十 GB，`deps/` 中出现大量
`*.rcgu.o`、同名不同哈希的 `say_it-*` 测试程序与 `.pdb`，`incremental/` 下有上百个
`say_it-*` 目录。目录的逻辑大小还可能因硬链接明显高于实际磁盘占用。

## 原因

1. 调试信息拆分：Cargo 的 macOS 开发配置默认启用完整调试信息和 `unpacked` 拆分模式，
   每个代码生成单元都会保留带调试信息的对象文件。
2. 旧代产物不回收：工具链升级、依赖版本或特性变化都会生成新的产物哈希，旧的那一代
   原样留在 `target/` 里，Cargo 不会自动清理。Windows 上实测 94 GB 中大部分是改用下方
   配置之前留下的旧产物，新鲜的一代开发构建约 4 GB。

## 项目约定

根包的开发配置使用有限调试信息和 `packed` 模式：

```toml
[profile.dev]
debug = 1
split-debuginfo = "packed"
```

这样仍保留开发态栈回溯和基本调试能力，同时避免为每个编译单元散落并重复链接对象文件。
不再继续压缩依赖的调试信息或关闭增量编译：节省有限，却会牺牲调试和增量重编速度。

升级 Rust 工具链或大批量更新依赖后，旧产物全部失效，用 Cargo 清理 dev profile 后重新构建：

```bash
cargo clean --manifest-path src-tauri/Cargo.toml --profile dev
```

不要清理 release profile：`tauri.windows.conf.json` 的打包资源引用
`target/release/*.dll`，清掉后需要先完成一次 release 构建才能重新打包。
也不要手工选择性删除 `deps/` 或 `incremental/`，以免留下不一致缓存。

## 验证

清理或改配置后执行一次完整开发构建，确认：

- 构建成功；
- `target/debug/deps` 不再生成 `*.rcgu.o`；
- 调试符号集中在 macOS 的 `.dSYM` 或 Windows 的 `.pdb` 中；
- 新鲜开发构建的 `target/debug` 维持在项目依赖规模对应的数 GB，而不是数十 GB。
