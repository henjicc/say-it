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
