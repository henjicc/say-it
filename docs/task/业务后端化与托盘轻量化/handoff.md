# 交接

## 1.1 → 后续任务

- `get_app_snapshot` 已注册，当前汇总 Rust 配置摘要、转写任务状态，并将其余领域标为 `frontendOwned`。
- 后续 1.2 应扩充同一 `AppSnapshot`，不得另建并行快照结构；每次可观察状态变化推进 revision。
- 后续事件统一复用 `DomainEventEnvelope`；窗口端采用两次快照夹事件订阅的校正流程。
- 1.1 自动验证已通过，可以进入 1.2；人工回归、资源及时延采集已移交 `manual-test-checklist.md`，在全部任务完成后的最终验收统一执行。
- 不要根据自动构建通过推定快捷键、注入、字幕编辑器或真实 ASR 正常。
