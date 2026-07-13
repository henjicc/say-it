# 修改文件

## 2026-07-13 · 1.1

- `src-tauri/src/application/mod.rs`：注册应用层契约模块。
- `src-tauri/src/application/contract.rs`：快照、领域状态、事件信封、错误 DTO、最小端口与契约测试。
- `src-tauri/src/state.rs`：增加快照 revision 原子计数器。
- `src-tauri/src/main.rs`：注册应用模块和 `get_app_snapshot` 命令。
- `ui/src/lib/tauri.ts`：增加命令常量及 TypeScript 契约镜像。
- `scripts/测量进程内存.ps1`：递归采集进程树工作集、私有字节、CPU 和进程数。
- `docs/task/业务后端化与托盘轻量化/` 内计划与记录：同步阶段状态、决策、交接、测试及门禁。
- `docs/task/业务后端化与托盘轻量化/manual-test-checklist.md`：集中维护全部任务完成后统一执行的人工功能、性能及时延验收项。
