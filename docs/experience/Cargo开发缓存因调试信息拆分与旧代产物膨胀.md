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

旧代产物由 `scripts/rust-cache-guard.mjs` 自动回收，只管理默认的 `src-tauri/target`，
CI 和设置了 `CARGO_TARGET_DIR` 时跳过：

- `npm run tauri:dev` 与 `npm run test:rust` 启动前检查：`rustc -vV` 与上次记录
  （`target/.sayit-rustc-version`）不同，或 `target/debug` 超过 20 GiB，就执行
  `cargo clean --profile dev`，代价是下一次 debug 构建从头编译。逐个统计文件在 Windows 上
  较慢，体积每天最多量一次；工具链每次都检查。同一次检查还会删除与 `Cargo.lock` 中
  `sherpa-onnx-sys` 版本不同的 `target/sherpa-onnx-prebuilt` 条目。
- 本地 release 只保留最新一次构建与安装包：`npm run tauri:build` 先把旧 release 改名为
  `target/release.previous-<打包进程 PID>`，打包成功后改名为 `release.trash-<PID>` 再删除，
  失败则原样恢复。打包被中断时，下一次 `tauri:dev`、`test:rust` 或打包启动会自动恢复；
  那次打包仍在运行时不动它的目录，并拒绝同时再打包。`--debug`、`--target`、`--help` 不触发这一步。
- 代价：打包期间新旧两份 release 并存，磁盘峰值约为 release 的两倍；每次本地打包和
  失败后的重试都从零编译 release。

旧 release 不能直接清空：dev 构建（tauri-build）会复制 `tauri.windows.conf.json` 引用的
`target/release/*.dll` 和 `tauri.macos.conf.json` 引用的 frameworks，缺失就无法编译。
macOS 的 `target/swift-module-cache` 不在回收范围内。直接运行 `cargo` 不经过这道检查；
不要手工选择性删除 `deps/` 或 `incremental/`，以免留下不一致缓存。

## 验证

清理或改配置后执行一次完整开发构建，确认：

- 构建成功；
- `target/debug/deps` 不再生成 `*.rcgu.o`；
- 调试符号集中在 macOS 的 `.dSYM` 或 Windows 的 `.pdb` 中；
- 新鲜开发构建的 `target/debug` 维持在项目依赖规模对应的数 GB，而不是数十 GB。
