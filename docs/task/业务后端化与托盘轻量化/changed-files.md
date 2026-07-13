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

## 2026-07-13 · 1.2

- `src-tauri/src/application/settings.rs`：版本化配置、旧数据导入、领域更新、自定义提示音落盘及 revision 推进。
- `src-tauri/src/application/{mod.rs,contract.rs}`、`src-tauri/src/state.rs`、`src-tauri/src/main.rs`：注册配置服务、权威状态、快照字段和命令。
- `src-tauri/src/persistence.rs`：schema、原子保存、备份替换、损坏恢复及测试。
- `ui/src/features/settings/settingsBridge.ts`、`ui/src/App.tsx`：启动导入与快照 hydrate。
- `ui/src/lib/tauri.ts`、四个配置 store、`SettingsMicCuePanel.tsx`：命令代理、成功后缓存及旧键兼容镜像。
- `docs/task/业务后端化与托盘轻量化/`：同步计划、状态、决策、交接和测试记录。

## 2026-07-13 · 2.1

- `src-tauri/src/application/catalog.rs`：版本化模型/供应商目录及完整性测试。
- `src-tauri/src/providers/{registry.rs,mod.rs}`、`commands/common.rs`：公开模型数据，输出有效能力、配置字段和 action。
- `src-tauri/src/{application/mod.rs,main.rs,prelude.rs}`：注册目录模块与命令。
- `ui/src/features/asr/{modelRegistry.ts,modelOptions.ts}`、`features/compare/models.ts`：改为消费后端目录，移除 JSON import 与协议推断。
- `ui/src/App.tsx`、`features/settings/settingsBridge.ts`、`store/useProviderStore.ts`、`views/SettingsProviderPanel.tsx`、`lib/tauri.ts`：启动目录加载门禁、供应商目录 hydrate、action 驱动面板和命令契约。
- `docs/rules/新增供应商与模型操作手册.md` 与 TASK_DIR 记录：同步新目录机制和验收状态。
