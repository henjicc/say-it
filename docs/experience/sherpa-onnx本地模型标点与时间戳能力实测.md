# sherpa-onnx 本地模型标点与时间戳能力实测

针对 `official.sherpa-paraformer-online`（流式）与 `official.sherpa-sensevoice-small`（整句/文件）
两个官方模型包的实测结论。测试音频为官方 `test_wavs/zh.wav`（5.59s，中文"开放时间早上九点至下午五点"）。

## 结论速查

| 能力 | Paraformer 流式 | SenseVoice |
| --- | --- | --- |
| 标点 | 不支持（词表无标点 token） | 不支持（词表有 token 但实测不产出） |
| 时间戳 | **不返回**（空数组） | **返回 token 级时间戳** |
| 中间结果 | 有（真流式） | 无（VAD 分段整句） |

## 标点：两个模型都不输出，是模型限制不是配置问题

词表检查：

- Paraformer `tokens.txt` 8404 条，无中文标点 token → 结构性不可能输出标点。
- SenseVoice `tokens.txt` 25055 条，**含**中文标点 token（`、` `。` `！` `，` `：` `；` `？`），
  另有 171 条 `<|...|>` 标签 token（含 `withitn`）。

但 SenseVoice 实测在以下四种组合下**均无任何标点**：

- 单句音频 + `use_itn=true` / `use_itn=false`
- 三句拼接（每句间插 0.4s 静音）+ `use_itn=true` / `use_itn=false`

即词表里有标点能力，但该 int8 checkpoint 实际不产出。

`use_itn` 控制的是逆文本规范化（数字/单位写法），**不是标点开关**；本例中它对文本无可见影响。

补记一个未定性的观察：单句直读时 `use_itn=true` 输出 `放时间早上九点至下午五点`（丢首字"开"），
`use_itn=false` 完整；但三句拼接时两者都完整。更像音频开头缺引导静音的边界效应，不是 ITN 的
确定性缺陷，暂不据此改默认值。

### 若要加标点

sherpa-onnx 的官方做法是外挂独立标点模型（ct-transformer），`sherpa-onnx` crate 已提供
`OfflinePunctuation::add_punctuation()`。需要新增一个标点模型包并在识别后处理链上调用，
属于新功能。

## 时间戳：SenseVoice 支持，Paraformer 不支持

直接调用识别器的实测返回：

```
[SenseVoice use_itn=false] text="开放时间早上九点至下午五点"
   tokens=13 项
   timestamps=Some([0.6, 0.9, 1.2, 1.44, 1.86, 2.1, 2.52, 2.82, 3.24, 3.9, 4.2, 4.5, 4.74])

[Paraformer online] text="菜放时间早上九点至下午五点"
   timestamps=Some([])          // 空数组
```

SenseVoice 的时间戳数量与 token 数一一对应，可用。

### 宿主一度把它丢掉了（已修复）

早期实现里 `LocalAsrOutput` 只有文本字段，本地文件识别固定返回 `"sentences": []`，
两个本地模型因此都只能声明 `supportsAlignmentTimestamps: false`。

现在 `recognize_file_segments` 返回 `LocalSegment { text, begin_ms, end_ms }`，时间轴取自
**VAD 句段边界**（而非 token 时间戳）——句级边界正是字幕和文稿对齐需要的粒度，且 online
引擎没有 token 时间戳，用句段边界可以让两种引擎走同一条路。SenseVoice 文件模型据此声明
`supportsAlignmentTimestamps: true`。

实现要点：`SpeechSegment::start` 只在**本轮 VAD 内**计数，`recognize_file_segments` 每分钟
会 `flush_and_reset` 一次，reset 后该计数归零。因此 `OfflineVadSession` 必须自己累计
`base_samples` 补齐绝对位置，否则长音频的时间轴会周期性回退或整体压缩到第一分钟内。
回归用例对十分钟音频断言"末句起点必须超过 9 分钟"，专门盯这个偏移。

> 注意：`supportsAlignmentTimestamps` 写在模型包清单里且已签名，**改描述文件后必须重新
> 构建并安装模型包**才会生效；`emitsPartialResults` 则有宿主按协议兜底，无需重装。

## 复现方式

模型权重在数据根的 `models/` 下（数据根可能被 `data-root.json` 指到自定义位置）。把官方
`test_wavs/zh.wav` 复制成模型目录下的 `test.wav`，再跑 `providers::local_asr::tests` 里带
`#[ignore]` 的 PoC 用例，验证完记得删除临时 wav。

词表检查可直接 grep：

```bash
grep -cP '^[，。！？、；：]' <模型目录>/tokens.txt
```
