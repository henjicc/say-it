# Gemini Live Translate 连续模型不发 turnComplete，字幕只显示原文没有译文

## 现象

实时字幕选择「Gemini 3.5 Live Translate（实时翻译）」后，悬浮窗只滚动日语原文，
译文行始终为空。前两次修复（`SubtitlePrefs::translation_enabled()` 对自带翻译的
识别模型返回 true、`render()` 不再因翻译模型为"无"而隐藏译文行）都只是让译文行
"有资格显示"，但连接器根本没有发出过 `translation` 事件。

## 根因

`gemini_session.rs` 原先只在 `generationComplete` / `turnComplete` / `interrupted`
到达时才把本轮累计的 `outputTranscription` 以 `translation(final)` 发出，译文片段
到达时只累积、不发草稿。而 Gemini 的 live-translate 是**连续模型**：

- 它可以整段对话都不发 `turnComplete`，甚至在 `audioStreamEnd` 之后也不发
  （Mimi 项目 `docs/development/common-regressions.md` 对真实端点的结论一致）。
- `generationComplete` 只表示模型本次生成结束，译文转写可能仍在路上；
  `turnComplete` 之后也仍可能继续送转写尾巴，边界只能当作临时的。
- `inputTranscription` / `outputTranscription` 既可能是增量片段，也可能是整段累计
  快照，不能一律 `push_str` 拼接。

于是译文永远停留在连接器内部缓冲里，原文则因为 `result(final:false)` 的部分结果
一直有输出而显示正常——表现为"只有原文"。

## 处理

连接器层（`src-tauri/src/commands/asr/gemini_session.rs` 的 `TurnAssembler`）按
Mimi 已验证的编排方式重写：

- 原文、译文每次变化都立刻发 `result` / `translation` 的 `final: false` 累计草稿；
- `turnComplete` / `interrupted` 只是临时边界，转写安静 500ms 后才定稿；
  `generationComplete` 不是边界；
- 没有显式边界时，两路都已有内容且安静 2s 就按本地检查点定稿这一对；
  只有一路有内容时多等到 5s 再单路定稿，字幕不会无限悬挂；
- 定稿顺序固定为先 `result(final)` 再 `translation(final)`；
- Finish 后：无待定稿内容且（收到过服务端边界或安静 2s）即收尾，否则到 8s 预算
  强制定稿。

协议层（`providers/gemini_live.rs` 的 `TranscriptLane`）按内容判别片段/累计快照：
严格扩展当前缓冲的报文视为快照整体替换，与缓冲相同的快照视为重发忽略，其余按
片段原样追加；本地取走过的前缀在后续快照里剥掉。

字幕层（`application/subtitles/mod.rs`）：

- `TranslationDocument.draft` 保存自带翻译模型的未定稿译文，和当前未定稿原文并排
  展示；
- `translation(final)` 挂到最近一轮定稿原文的第一个分段序号上；该分组已回填过时
  视为迟到尾巴追加而非覆盖；没有任何可挂靠的原文分段时保留为草稿。

## 验证方式

- 协议层与 `TurnAssembler` 的时序逻辑有单元测试覆盖（草稿即时发出、无
  `turnComplete` 的安静定稿、`turnComplete` 后吸收迟到尾巴、单路超时定稿）。
- 字幕层 `apply_live_translation` 有单元测试覆盖草稿展示、定稿回填、尾巴追加、
  无挂靠点保留草稿。
- 真机验证：选 Gemini Live Translate 作为字幕识别模型、翻译模型保持"无"，播放日语
  音频，译文行应先出现增量草稿，停顿后原文与译文同时定稿；停止字幕应在 2s 内收尾。

## 教训

- 接入"自带翻译"的实时模型时，不要假定供应商一定提供轮次终止事件；译文必须有
  独立于边界事件的展示（草稿）与定稿（安静检查点）路径。
- 排查"译文不显示"时先确认连接器是否真的发出了 `translation` 事件（开启调试日志看
  `[asr xxxx] translation ...`），再去查展示层的开关。
