# 测试报告

## 2026-07-13 · 1.1

### 自动验证

- `npm run ui:build`：通过，Vite 双入口构建成功。
- `cargo test application::contract`：通过，2/2。
- `cargo check`：通过。
- `scripts/测量进程内存.ps1`：待在运行中的桌面应用上采集真实数据；输出包含根进程及递归子进程明细、汇总工作集/私有字节、5 秒 CPU 百分比和进程数。

### 人工验证安排

- 用户决定所有人工测试统一推迟到全部任务完成后执行；完整项目、步骤、数据记录要求统一维护在 `manual-test-checklist.md`。

### 当前结论

- 自动化部分通过。
- 1.1 自动验收通过，任务已完成；人工行为、四类资源和两项时延保留为最终验收待办，不阻塞后续任务。

## 2026-07-13 · 1.2

### 自动验证

- `cargo test --manifest-path src-tauri/Cargo.toml`：通过，69/69。
- `cargo check --manifest-path src-tauri/Cargo.toml`：通过。
- `npm run ui:build`：通过，Vite 主窗口与 indicator 双入口构建成功。
- `git diff --check`：通过，仅有 Git 的 LF/CRLF 工作区提示。

### 未自动验证

- 真实历史数据升级、重启恢复、设置页交互和自定义提示音试听需桌面 UI 操作，统一列入 `manual-test-checklist.md`。

### 当前结论

- 1.2 自动验收通过；人工项目不阻塞后续阶段。

## 2026-07-13 · 2.1

### 自动验证

- `cargo test --manifest-path src-tauri/Cargo.toml`：通过，71/71。
- `cargo check --manifest-path src-tauri/Cargo.toml`：通过。
- `npm run ui:build`：通过，Vite 主窗口与 indicator 双入口构建成功。
- 源码/构建产物扫描：前端无 `asr-models.json` import，无按协议或模型名前缀判断。
- `git diff --check`：通过，仅有 Git 的 LF/CRLF 工作区提示。

### 当前结论

- 2.1 自动验收通过；页面选项和供应商设置交互加入最终人工清单，不阻塞 2.2。
