# cargo check 不要与 ui build 并行

## 现象

`cargo check` 会通过 `tauri::generate_context!()` 读取 `ui/dist` 中的前端构建产物；`npm run ui:build` 会先清理并重新生成这些文件。两者并行执行时，Cargo 可能刚好读到被删除的旧 hash 文件，报 `couldn't read ../ui/dist/...`。

## 处理

- 需要同时验证 Rust 和前端构建时，先执行 `npm run ui:build`，再执行 `cargo check`。
- 不要把 `cargo check` 和会重建 `ui/dist` 的命令放进同一个并行工具调用。
