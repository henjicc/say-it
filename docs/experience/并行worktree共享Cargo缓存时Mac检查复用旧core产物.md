# 并行 worktree 共享 Cargo 缓存时 Mac 检查复用旧 core 产物

## 识别条件

多个 worktree 共用主检出的 target 时，`check:mac-overlay` 可能复用另一个 worktree 的 core 检查产物，报告当前源码中明确存在的公共类型不存在。先确认模块声明、依赖路径与源码，不要为缓存造成的缺失错误修改公共契约。

## 处理

只清理 mac-check 中对应目标和包的产物，再重新检查：

```powershell
$env:CARGO_TARGET_DIR='D:\VibeCode\说吧\src-tauri\target'
cargo clean --manifest-path src-tauri/Cargo.toml -p sayit-overlay-core --target aarch64-apple-darwin --target-dir 'D:\VibeCode\说吧\src-tauri\target\mac-check'
npm run check:mac-overlay
```

`cargo clean -p` 必须同时指定 `--target aarch64-apple-darwin`；只指定 mac-check 的目录仍按宿主目标清理，可能显示 `Removed 0 files`，未清到交叉检查产物。不删除整个共享 target，也不另建仓库外缓存。
