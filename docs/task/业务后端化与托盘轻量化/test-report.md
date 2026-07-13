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

## 2026-07-13 · 2.2

### 自动验证

- `cargo test --manifest-path src-tauri/Cargo.toml`：通过，76/76；含能力缺失、fake 同步完成/异步进度、取消迟到结果、翻译增量与流错误。
- `cargo check --manifest-path src-tauri/Cargo.toml`：通过，无 warning。
- `npm run ui:build`：通过，Vite 主窗口与 indicator 双入口构建成功。
- 命令层扫描：转写、翻译、热词业务命令不再直接调用 `providers::alibabacloud`。
- `git diff --check`：通过，仅有 Git 的 LF/CRLF 工作区提示。

### 当前结论

- 2.2 自动验收通过；真实文件识别、翻译和热词云端行为加入最终人工清单，不阻塞 3.1。

## 2026-07-13 · 3.1（未通过）

### 自动验证

- 临时原型 `cargo test application::audio_session`：通过，2/2。
- 临时原型 `cargo test application::dictation`：通过，4/4。
- 临时原型 `cargo check`：通过，但有新增结构未接入生产路径产生的 dead_code warning；因此原型已撤回，不作为交付验证。

### 失败原因

- 自动测试只验证了领域基础；真实快捷键仍向 WebView 发旧事件，麦克风、ASR、文件识别、规则、提示音、注入和指示器仍由前端编排。
- 前端仍持有 session ID、epoch、计时器和 PCM 缓冲，不满足 3.1 验收标准，故不得进入下一阶段。
- 第二次沿内部函数提取路径分析发现：`start_asr_stream`、`transcription_start`、`start_backend_mic` 等均直接接收 `tauri::State` 并只向 WebView 广播完成事件；若只提取启动函数，Rust 应用服务仍收不到 ASR/转写完成事件；若同时改事件回调，则会一次跨越 ASR、转写、麦克风、快捷键和前端五条运行路径，无法小步保证唯一权威。
- 本地规则使用 JavaScript RegExp + Worker 强制超时；Rust 当前无 ECMAScript 正则引擎，直接使用 Rust regex 会破坏回溯、反向引用、前后查找等用户规则语义。提示音虽有 Symphonia 解码，但没有通用输出播放层。这两项都需要明确依赖/兼容方案后才能无损切换。

## 2026-07-13 · 3.1（恢复验收通过）

### 自动验证

- `cargo test --manifest-path src-tauri/Cargo.toml`：通过，82/82；新增覆盖内部事件无 WebView 分发、租约冲突/过期/延迟释放 generation、epoch、单次注入约束、反向引用与前后查找规则。
- `cargo check --manifest-path src-tauri/Cargo.toml`：通过，无 warning。
- `npm run ui:build`：通过，Vite 主窗口与 indicator 双入口构建成功。
- 源码扫描：前端听写目录不再存在 session ID、ASR epoch、收尾/静音计时器或 PCM 缓冲；旧五个运行时文件已删除。
- `git diff --check`：通过，仅有 Git 的 LF/CRLF 工作区提示。
- `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`：仓库既有 Rust 文件整体未按 rustfmt 排版，检查会报告大量任务外差异；本阶段未为通过格式检查而扩大改动范围，不作为项目现有门禁。

### 未自动验证

- 真实全局快捷键、麦克风输入/输出、云端实时与文件识别、文本注入、用户自定义规则和主窗口临时销毁均涉及实际设备/UI/外部服务，按用户决定统一列入 `manual-test-checklist.md`。

### 当前结论

- 3.1 自动验收通过；人工项目保留到全部任务完成后统一执行，可以进入 3.2。

## 2026-07-13 · 3.2

### 自动验证

- `cargo test --manifest-path src-tauri/Cargo.toml`：通过，87/87；新增覆盖替换模式 2.5 秒续接/超时重开、滚动行裁剪、标点/逗号/60 字强制分句、翻译乱序重建及 epoch 隔离。
- `cargo check --manifest-path src-tauri/Cargo.toml`：通过，无 warning。
- `npm run ui:build`：通过，Vite 主窗口与 indicator 双入口构建成功。
- 源码扫描：前端字幕目录和 Tauri bridge 不再存在真实 session ID、translation request/epoch、重连计数器、OBS monitor 或 `backend-*-raw-chunk` 消费。
- `git diff --check`：通过，仅有 Git 的 LF/CRLF 工作区提示。

### 未自动验证

- 真实麦克风/系统 loopback、云端 ASR/翻译、OBS Browser Source、字幕悬浮窗拖动和主 WebView 销毁后的连续运行涉及设备、外部服务或 UI 操作，按用户决定统一列入 `manual-test-checklist.md`。

### 当前结论

- 3.2 自动验收通过；真实设备、网络、OBS 和交互项目不阻塞 3.3。

## 2026-07-13 · 3.3

### 自动验证

- `cargo test --manifest-path src-tauri/Cargo.toml`：通过，93/93；新增覆盖静默初始窗口、重复打开只创建一次、创建失败可重试、关闭状态边界、负坐标副屏和断屏回退。
- `cargo check --manifest-path src-tauri/Cargo.toml`：通过，无 warning。
- `npm run ui:build`：通过，Vite 主窗口与 indicator 双入口构建成功。
- `npx tsc --noEmit --lib ES2022,DOM,DOM.Iterable`：通过；项目默认 `npx tsc --noEmit` 仍被既有 ES2020 配置与 `Array.prototype.at` 冲突阻塞，错误位于未改动的 `features/subtitles/controller.ts:135`。
- 生命周期静态复查：关闭路径不调用 `dictation_stop`、`subtitle_stop` 或 OBS shutdown；托盘菜单、托盘左键和单实例均调用 `ensure_main_window`；前端不再注册 `beforeunload` 业务清理。
- `git diff --check`：通过，仅有 Git 的 LF/CRLF 工作区提示。

### 未自动验证

- 主 renderer 实际退出、连续托盘点击、关闭/恢复动画、位置/尺寸/最大化、多屏 DPI、全局快捷键及字幕/OBS 后台连续性需要桌面 UI 或外部服务，统一列入 `manual-test-checklist.md`。
- 前台、托盘 10/60/300 秒、听写后和字幕运行中的进程树资源与恢复时延需在真实构建运行时采样，统一留到最终人工验收。

### 当前结论

- 3.3 自动验收通过；人工窗口、后台连续性和性能项目按用户决定不阻塞 4.1。

## 2026-07-13 · 5.1

### 自动验证

- `npm run ui:build`：通过，Vite 主窗口与 indicator 双入口构建成功。
- `cargo check --manifest-path src-tauri/Cargo.toml`：通过，无 warning。
- `cargo test --manifest-path src-tauri/Cargo.toml`：通过，98/98。
- `npx tsc --noEmit --lib ES2022,DOM,DOM.Iterable`：通过。`cargo fmt --check` 未通过，原因是仓库既有大量未格式化文件；未执行全仓格式化，避免任务外差异。
- 静态扫描：前端未消费 `asr-stream-event`、`subtitle-translation-event`、`backend-*-raw-*`，未直接导入 `asr-models.json`，未保留 `funasr_*` 或旧 ASR stream 公共命令；后端对应 Tauri 注册已删除。
- fake provider 演练：`providers::testing` 的无网络测试覆盖协议无关文件成功/异步进度、取消后的迟到完成、翻译增量与错误；业务状态机未按供应商分支。

### 未自动验证

- 必须由用户完成 `manual-test-checklist.md` 的真实设备、快捷键、窗口、OBS、升级回退、资源和时延项目；该限制由桌面 UI 与外部服务决定。

### 当前结论

- 5.1 代码和自动验收通过，整体计划处于待验证状态。
