# Apple 流式语音识别结果不是全文快照

## 触发条件

- 旧版 `SFSpeechRecognizer` 连续识别约一分钟，内部窗口滚动后，新的 partial 可能只包含后续片段，并且不会先为旧窗口发送 final。
- macOS 26 `SpeechTranscriber` 的每个 `Result` 对应一个音频范围；volatile 结果会被同范围的新结果替换，final 结果才可追加。
- 应用因静音策略主动关闭识别流时，供应商可能还没有发送 final。

把所有流式结果都当作“本次会话完整全文”并直接覆盖当前文本，会在上述任一路径丢掉前文。

## 正确做法

- Apple 原生 Helper 对外统一输出会话级完整快照，平台特有的结果语义不得泄漏到通用听写状态机。
- `SFSpeechRecognizer` 使用 `SFTranscriptionSegment.timestamp` 识别内部窗口滚动；滚动前保存上一窗口的最后快照，再开始维护新窗口。部分系统时间戳会重置，需要用文本大幅缩短作为兜底信号。
- `SpeechTranscriber` 分开维护 finalized 与 volatile：finalized 依次追加，volatile 只替换当前尾部；整个 analyzer 完成后才向通用协议发送会话 final。
- 静音断开前提交最后一个可见 partial，再关闭旧流，不能直接清空。
- 回归测试至少使用超过 60 秒的连续音频，并检查最终文本同时保留开头和结尾；另测超过静音阈值的停顿。
