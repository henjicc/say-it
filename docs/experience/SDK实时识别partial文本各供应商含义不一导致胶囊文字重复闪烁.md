# SDK 实时识别 partial 文本各供应商含义不一，导致胶囊文字重复闪烁

## 现象

用火山 seedasr-2.0-realtime 听写，说到第一句定稿（约第三行）之后，胶囊文字突然多出几行并快速滚动；下一句定稿时又一下缩回，看起来像整段消失再出现。最终粘贴的文字正常。

## 原因

听写状态机（`application/dictation/delivery.rs`）约定：partial 是**当前这一句**，替换 `segment`；final 把这一句提交进 `committed`。显示文字 = `committed + segment`。

`@henjicc/ai-sdk` 的 partial `text` 在不同供应商里含义不同：

- 百炼：当前这一句。
- 火山实时：整场累计全文（`result.text`），当前这一句另外放在 `partial.segment`。

所以用火山时，已定稿的句子会在显示里再出现一遍；直到下一句定稿，`segment` 被替换成单句，显示才塌回正确长度。最终文字由 final 拼成，因此不受影响。

## 做法

SDK 到宿主的适配层（`commands/asr/sdk_session.rs` 的 `partial_text`）在 partial 带 `segment` 时取 `segment.text`，没有时才用 `text`。插件运行时的 partial 只有 `text`，约定本来就是当前句，不受影响。

## 排查方法

开发者追踪默认不记录每条实时结果（量大，火山累计全文会让记录随听写时长平方增长）。需要时临时在 `DictationRuntime::record_asr_event` 的 `result` 分支调用 `dev_trace::mark`，内容字段用 `rawText`，这样敏感输入仍会被清除。看每条的文本长度与显示总字数：定稿后显示总字数突然大于实际说的字数，就是 partial 含义不一致。打印时不要只截取文本末尾，否则会误以为供应商截掉了开头。
