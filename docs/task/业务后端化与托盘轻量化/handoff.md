# 交接

## 1.1 → 后续任务

- `get_app_snapshot` 已注册，当前汇总 Rust 配置摘要、转写任务状态，并将其余领域标为 `frontendOwned`。
- 后续 1.2 应扩充同一 `AppSnapshot`，不得另建并行快照结构；每次可观察状态变化推进 revision。
- 后续事件统一复用 `DomainEventEnvelope`；窗口端采用两次快照夹事件订阅的校正流程。
- 1.1 自动验证已通过，可以进入 1.2；人工回归、资源及时延采集已移交 `manual-test-checklist.md`，在全部任务完成后的最终验收统一执行。
- 不要根据自动构建通过推定快捷键、注入、字幕编辑器或真实 ASR 正常。

## 1.2 → 后续任务

- `AppSnapshot.settings` 已成为配置权威投影；新窗口通过旧数据幂等导入后重新拉快照并 hydrate 四个 Zustand store。
- 后续领域配置修改应复用后端设置命令或收敛为更具体的 Rust 命令，不得恢复 localStorage 权威。
- 领域文档目前保留未知字段；2.x/3.x 接管具体业务校验时逐领域替换为 Rust 强类型。
- 自定义提示音已写入应用数据目录；3.1 应从该文件原生播放，完成前旧 Data URL 镜像不得删除。

## 2.1 → 后续任务

- `get_model_catalog` 是模型元数据、场景选项、默认模型和供应商描述的唯一前端入口；2.2 不得新建前端能力映射。
- 供应商目录已输出 `effectiveCapabilities`、`configFields`、`actions`；复杂热词能力由 `manageHotwords` action 声明。
- 前端启动必须先加载目录再 hydrate 配置；目录加载失败时业务页面不会进入可操作状态。

## 2.2 → 后续任务

- 文件识别统一从 `providers::capabilities::file_recognition_for` 获取；翻译使用 `translation_for`；热词使用 `customization_for`，上层不得直接调用 `providers::alibabacloud`。
- 当前 facade 有意使用枚举而非插件 trait；新增真实供应商时在工厂增加分支并实现相同方法，不改听写、字幕、转写或对比状态机。
- `provider_*_hotwords` 是新通用入口，旧 `funasr_*` 仅兼容代理，5.1 可在确认无调用后删除。
- 真实网络与 UI 验证统一在最终人工清单执行；后续阶段不可据自动测试推断阿里云在线接口已验证。

## 3.1 阻塞交接

- owner/generation lease 与 epoch 状态机已通过临时原型验证，但未接入原型已撤回，避免后续误认为存在可用基础。
- 下一次只能继续 3.1：先将现有底层命令拆成可由应用服务直接调用的内部函数，再让快捷键调用服务，最后切换前端为快照/事件消费者。
- 接入前不得移除既有前端流程；切换时必须一次完成唯一权威，避免双重响应快捷键和重复注入。
- 具体技术阻塞：底层命令只向 Tauri/WebView 事件发布结果，缺少供应用层订阅的 Rust 内部事件总线；应先单独增加后端事件分发端口，并让旧 WebView 广播成为该端口的适配器，再迁移听写。
- 规则迁移需先选择 ECMAScript 兼容引擎或明确拒绝规则的迁移策略；提示音需增加原生输出播放边界。两项均不能用静默跳过或 WebView 回退冒充完成。

## 3.1 → 3.2/3.3/4.2

- `RuntimeState.audio_session` 是共享麦克风 owner/generation 权威；3.2 字幕、4.2 对比/音频调试应使用各自 `AudioOwner`，不要绕过协调器直接开设备。
- `BackendEventHub` 是 Rust 内部异步结果入口；3.2 复用它消费 ASR/翻译结果，WebView 广播只作为迁移期兼容输出。
- 听写快捷键已直接驱动 Rust，前端只调用 `dictation_*` 命令和消费 `domain-event`；不得恢复 `dictation-toggle` 的前端状态机监听。
- 指示器现在由后端发状态、文本和摘要波形；完整 PCM 仅在旧字幕/对比/调试路径按需广播，3.2/4.2 应继续移除各自广播消费者。
- 真实设备、云端、规则样本和提示音交互仍须最终人工清单验证；自动验证通过不代表外部服务实测通过。

## 3.2 → 3.3/4.2/5.1

- `RuntimeState.subtitle_runtime` 是真实字幕、翻译和 OBS 路由唯一权威；3.3 销毁主窗口时不得调用 `subtitle_stop`，窗口重建后用 `get_subtitle_runtime` 加 `domain-event` 恢复投影。
- 字幕快捷键已直接调用 Rust `request_toggle`，主 WebView 不再监听 `subtitle-toggle`；不要恢复前端快捷键状态机。
- 麦克风与系统音频都使用 `AudioOwner::Subtitles`；4.2 必须使用自己的 owner，不能借用 Legacy 或绕过协调器。
- 前端 `features/subtitles/controller.ts` 只保留命令、状态投影、热键设置和完全隔离的预览；真实 ASR、translation epoch、重连、OBS monitor 与 PCM 消费均已删除。
- 旧 `asr-stream-event`、`subtitle-translation-event`、`translate_subtitle_start` 和底层 raw capture 命令仍可能被 4.2/调试页使用，5.1 须先扫描消费者再清理。
- 真实设备、云端翻译、OBS、拖动与窗口销毁连续性统一留在 `manual-test-checklist.md`，自动验证不能替代这些结果。

## 3.3 → 4.1/4.2/5.1

- `desktop::ensure_main_window` 是托盘、单实例和后续显式打开主窗口的唯一入口；不要直接 `show()` 或另建 `main` WebView。
- 主窗口关闭现在调用 `destroy_main_window`，只保存窗口状态并销毁 WebView；4.1/4.2 的后台任务不能依赖 `beforeunload` 收尾，必须由 Rust runtime 自己持有并通过命令显式取消。
- 新窗口在 `main_window_ready` 前保持不可见；`useTauriBridge` 已建立 revision 校正和听写/字幕完整运行时恢复，4.1/4.2 接管领域后需把其完整 runtime 投影加入同一恢复流程。
- `MainWindowPlacement` 保存逻辑内容尺寸、物理位置和最大化标记；多屏断开回退居中。不要恢复 outer size，避免无边框窗口反复变大。
- 指示器 WebView仍常驻，但空闲时原生窗口已隐藏；最终性能采样未证明需要进一步销毁前，不要增加指示器冷启动路径。
- 真实销毁后的进程数/内存、后台听写字幕连续性和窗口恢复体验统一见 `manual-test-checklist.md`，最终验收前不能仅凭单元测试宣称性能达标。

## 5.1 → 最终人工验收

- 已删除旧 `funasr_*`、raw PCM/ASR/字幕翻译 WebView 事件以及前端会话工具；新增功能不得重新注册这些命令或把后台会话放回 TypeScript。
- 请按 `manual-test-checklist.md` 验证真实音频、快捷键、窗口、OBS、升级回退和资源指标。通过后，将 5.1 和总览整体状态改为“已完成”；未通过时记录真实瓶颈，不通过增加兼容路径回避。
