# cargo fmt 全仓格式化污染

## 现象

在只改少量 Rust 文件时执行 `cargo fmt`，会把历史上未格式化或换行风格不同的 Rust 文件一并改动，导致 `git diff` 出现大量与当前任务无关的格式化变更。

## 处理

- 提交前必须用 `git status` / `git diff --stat` 检查是否出现无关 Rust 文件改动。
- 若只是格式化污染，应只保留本任务相关文件的改动，撤回其他 Rust 文件的格式化差异。
- 后续除非任务明确要求全仓格式化，避免在当前项目直接执行全仓 `cargo fmt`。
- 直接对 `main.rs`、`mod.rs` 执行 `rustfmt` 也会默认递归格式化子模块；局部格式化必须使用 `rustfmt --config skip_children=true <files>`，随后立即检查 `git diff --stat`。
