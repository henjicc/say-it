# 发布版本时同步更新 Cargo 与前端版本号

## 触发条件

发布桌面端新版本时，只更新了 `package.json` 和 `src-tauri/tauri.conf.json`，但 `src-tauri/Cargo.toml` 的 Rust 包版本仍停留在旧值，导致桌面包元数据出现版本漂移。

## 正确做法

每次发布前至少同步检查这四处版本号：

- `package.json`
- `package-lock.json`
- `src-tauri/tauri.conf.json`
- `src-tauri/Cargo.toml`

再补上 `CHANGELOG.md` 对应版本节，保证 GitHub Release、Tauri 打包配置和 Rust 包元数据一致。
