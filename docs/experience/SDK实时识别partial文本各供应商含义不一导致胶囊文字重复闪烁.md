# SDK 实时识别 partial 文本各供应商含义不一，导致胶囊文字重复闪烁

## 现象

用火山 seedasr-2.0-realtime 听写，第一句定稿后（约第三行）胶囊文字突然多出几行并快速滚动，下一句定稿时又一下缩回，看起来像整段消失再出现。绕开这一点后，每句交界处还会有最后一小段消失约 0.3 秒再出现。最终粘贴的文字正常。

## 原因

听写状态机（`application/dictation/delivery.rs`）约定：partial 是**当前尚未定稿的文字**，替换 `segment`；final 把刚定稿的一句提交进 `committed`。显示文字 = `committed + segment`。

`@henjicc/ai-sdk` 0.12.0 的火山实时模块不符合这个约定：

- partial `text` 取 `result.text`，是整场累计全文，于是已定稿的句子被再显示一遍；
- 同一帧里先发 partial 再发新定稿的 final。句子交界那一帧同时带有“下一句开头”和“上一句定稿”，后到的 final 把下一句开头盖掉，要等下一帧才恢复。

百炼也在同一批里先 partial 后 final，但那条 partial 属于同一句，所以宿主无法在不区分供应商的前提下修好顺序问题，只能回 SDK 修。

## 做法

SDK 0.13.0 在 `SpeechRecognitionEvent` 上写明契约：partial `text` = 所有未定稿文字，final `text` = 刚定稿的一句，每句只发一次，同一批结果先 final 后 partial；火山实时模块按此修正。说吧升级到 0.13.0 后，`commands/asr/sdk_session.rs` 直接读 partial 的 `text`，不做任何供应商特判。

以后接入新的实时识别供应商，先核对它是否满足这条契约；不满足就回 SDK 主仓修，不在宿主绕行。

## 排查方法

开发者追踪默认不记录每条实时结果：量大，遇到累计全文的供应商时，记录量会随听写时长成平方增长。需要时临时在 `DictationRuntime::record_asr_event` 的 `result` 分支调用 `dev_trace::mark`，内容字段用 `rawText`，这样敏感输入仍会被清除。看每条的文本长度与显示总字数：定稿后显示总字数突然大于实际说的字数，就是 partial 含义不对；同一毫秒内先出现下一句开头、再出现上一句 final，就是顺序不对。打印时不要只截取文本末尾，否则会误以为供应商截掉了开头。
