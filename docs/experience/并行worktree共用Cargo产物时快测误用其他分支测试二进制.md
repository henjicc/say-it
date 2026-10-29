# 并行 worktree 共用 Cargo 产物时快测误用其他分支测试二进制

## 现象与证据

多个 worktree 按项目约定共用主检出的 `src-tauri/target` 时，不能只凭 `cargo test` 返回成功判断测试属于当前分支。同一个包在不同 worktree 中可以写入同名测试二进制；一次快测显示 `Finished`、没有重新编译，却执行了另一工作树的测试集。

本次 `sayit-overlay-core` 应有 54 项测试，快测却执行了 52 项：包含其他工作树新增的 SVG 圆弧测试，缺少当前分支的 5 项胶囊模型测试。当前工作树源码没有变化，之前全量测试日志中这 5 项已通过。

## 处理与验证

出现测试数或测试名不符时，先保留日志，再刷新**当前 worktree、本任务已修改源码**的修改时间，促使 Cargo 重编该包。例如：

```powershell
$env:CARGO_TARGET_DIR='D:\VibeCode\说吧\src-tauri\target'
(Get-Item -LiteralPath 'src-tauri/crates/overlay-core/src/indicator.rs').LastWriteTimeUtc = [DateTime]::UtcNow
cargo test --manifest-path src-tauri/Cargo.toml -p sayit-overlay-core
```

确认日志的 `Compiling` 指向当前 worktree，并核对本次新增的具体测试名。刷新后本次重新执行 54 项，5 项胶囊模型测试均出现且通过。修改时间刷新不改变文件内容，不产生 Git 差异。

测试数只是线索，不能代替测试名核对；其他工作树可能恰好有相同数量的测试。并行编译同包时需要在可控构建时段确认结果，不能通过清空共享 target、关闭检查或另建完整 target 绕过项目约束。

## 本地依赖产物也需要核对

共享缓存还可能复用其他 worktree 的 path dependency。典型错误是 `image::DynamicImage` 与同版本 `image::images::dynimage::DynamicImage` 类型不兼容，编译诊断里的 `ocr-rs/src/engine.rs` 却指向另一 worktree。先核对诊断路径与依赖版本，确认是产物来源冲突后，只刷新当前 worktree 对应本地依赖源文件的修改时间，促使 Cargo 重建该依赖；不改文件内容、不清理共享 debug 目录，也不修改无关业务接口。重新运行时确认 `Compiling ocr-rs` 与 `Compiling say-it` 都指向当前 worktree。
