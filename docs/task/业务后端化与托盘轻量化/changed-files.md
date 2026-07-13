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

## 2026-07-13 · 2.2

- `src-tauri/src/providers/capabilities.rs`：文件识别、翻译、customization 能力 facade、工厂和结构化能力错误。
- `src-tauri/src/providers/testing.rs`：无网络 fake provider 标准事件测试。
- `src-tauri/src/providers/{mod.rs,alibabacloud/mod.rs,alibabacloud/transcription/mod.rs}`、`prelude.rs`：注册能力模块并公开必要标准类型。
- `src-tauri/src/commands/{transcription.rs,translation.rs,funasr.rs}`：业务命令改走通用能力，新增通用热词入口并保留兼容代理。
- `src-tauri/src/main.rs`、`ui/src/lib/tauri.ts`、`ui/src/store/useProviderStore.ts`：注册通用命令并让前端携带 provider ID 调用。
- `docs/rules/新增供应商与模型操作手册.md`：更新文件识别、翻译和 customization 的真实扩展步骤。
- `docs/task/业务后端化与托盘轻量化/`：同步阶段状态、决策、交接、测试及最终人工清单。

## 2026-07-13 · 3.1（阻塞现场）

- 源码：无最终保留改动；未接入原型已撤回，避免留下 dead_code 和半套运行时。
- `docs/task/业务后端化与托盘轻量化/`：记录阻塞原因、验证结果和后续接入顺序。

## 2026-07-13 · 3.1（恢复完成）

- `src-tauri/src/application/{events,audio_session,dictation}.rs`：内部事件总线、音频租约协调器与完整听写应用服务。
- `src-tauri/src/{state.rs,main.rs,hotkey.rs,application/{mod,contract,settings}.rs}`：注册运行时、命令、快照、快捷键直连和规则保存校验。
- `src-tauri/src/commands/{audio,asr/mod,common,dictation,transcription}.rs`、`desktop/backend_mic.rs`：提取应用层可调用内部能力，异步结果双发内部事件与兼容 WebView 事件，旧麦克风命令接入协调器。
- `src-tauri/{Cargo.toml,Cargo.lock}`：增加 MIT `fancy-regex 0.14.0`，用于有界回溯、反向引用和前后查找规则。
- `ui/src/features/dictation/controller.ts`、`hooks/useTauriBridge.ts`、`lib/{tauri,cues}.ts`：改为命令代理、领域事件投影和原生提示音试听。
- `ui/src/store/useDictPrefs.ts`、`views/SettingsMicCuePanel.tsx`、`features/settings/settingsBridge.ts`：配置保存改为可等待，确保自定义提示音落盘并切换为 custom 后才调用 Rust 试听。
- 删除 `ui/src/features/dictation/{session,realtimeFlow,fileFlow,inject,indicatorBridge}.ts`，并将仍供字幕/对比使用的 `micSession.ts` 移至 `features/audio/`：移除前端听写运行时、计时器和 PCM 缓冲。
- `docs/task/业务后端化与托盘轻量化/`：同步完成状态、决策、交接、测试与最终人工清单。

## 2026-07-13 · 3.2

- `src-tauri/src/application/subtitles/mod.rs`：字幕状态机、纯文档/翻译模型、音频与 ASR 生命周期、供应商翻译、OBS 路由、指示器更新及单元测试。
- `src-tauri/src/application/{events,contract,mod}.rs`、`state.rs`：内部翻译事件、字幕快照权威、模块和 runtime 注册。
- `src-tauri/src/desktop/backend_system_audio.rs`：补齐系统 loopback 供应用服务直接使用的 start/raw/ASR/pause/release 内部接口。
- `src-tauri/src/{hotkey.rs,main.rs}`：字幕快捷键直连 Rust 服务，注册初始化和运行时命令。
- `ui/src/features/subtitles/controller.ts`：删除真实字幕/翻译/OBS/PCM 编排，收敛为命令、领域投影、热键设置和隔离预览。
- `ui/src/hooks/useTauriBridge.ts`、`lib/tauri.ts`、`store/useSubtitleStore.ts`：消费字幕领域事件、注册命令、设置持久化后同步后端展示。
- `docs/task/业务后端化与托盘轻量化/`：同步 3.2 状态、决策、交接、测试与最终人工清单。

## 2026-07-13 · 3.3

- `src-tauri/src/application/window_lifecycle.rs`、`application/mod.rs`、`state.rs`：新增主窗口五态生命周期、幂等/失败恢复逻辑、代次保护及单元测试，并扩展位置尺寸/最大化状态。
- `src-tauri/src/desktop/window.rs`：实现统一按需创建、ready 后显示、保存与恢复逻辑内容尺寸、多屏位置校验、销毁及失败回退隐藏。
- `src-tauri/src/desktop/indicator.rs`：指示器初始不可见，`hidden` 状态真正隐藏原生窗口，显示态保持不抢焦点提升。
- `src-tauri/src/main.rs`：托盘菜单/点击、第二实例和启动路径接入统一生命周期；关闭请求改为销毁；注册 ready 命令。
- `src-tauri/Cargo.toml`、`Cargo.lock`：增加官方 `tauri-plugin-single-instance 2.4.2`。
- `ui/src/App.tsx`、`hooks/useTauriBridge.ts`、`lib/tauri.ts`：ready 握手、快照/订阅/revision 稳定校正及仅 UI 监听卸载。
- `ui/src/indicator/IndicatorApp.tsx`：字幕关闭直接调用 Rust `subtitle_stop`，无主窗口时仍生效。
- `docs/task/业务后端化与托盘轻量化/`：同步 3.3 计划、状态、决策、交接、测试和最终人工清单。

## 2026-07-13 · 5.1

- `src-tauri/src/{main.rs,commands/,desktop/{backend_mic,backend_system_audio}.rs,state.rs,prelude.rs}`：移除无消费者的迁移兼容命令、WebView raw PCM/旧 ASR 事件转发及其状态类型，保留 Rust 内部应用服务路径。
- `ui/src/lib/tauri.ts`、`ui/src/features/audio/{micSession,silenceDisconnect}.ts`：删除未使用的兼容命令/事件常量与旧前端音频会话工具。
- `docs/rules/新增供应商与模型操作手册.md`、`AGENTS.md`、TASK_DIR 记录：同步最终供应商、前端边界、兼容策略和自动/人工验收状态。
