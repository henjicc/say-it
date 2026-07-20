# sherpa-onnx VAD 喂入块过大与误 reset 导致无识别结果

## 问题现象

本地 SenseVoice（`sherpa-onnx-offline` 引擎）实时听写全程没有任何识别结果，事件流只有
`opened → finish → ended`，既不报错也不出文本。同为 sherpa-onnx 的 Paraformer
（`sherpa-onnx-online`）一切正常，容易误判为"同一推理框架不该有差异"。

关键差异在于：online 引擎直接把音频喂给识别器，offline 引擎多了一层 Silero VAD 负责切句，
问题全部出在 VAD 这一层，与 SenseVoice 模型本身无关。

## 根因

两个独立缺陷叠加，都在 `src-tauri/src/providers/local_asr/mod.rs`：

### 1. `drain()` 在语音未确认时 `reset()`（致命，实时路径全哑）

原实现每次 drain 结束都做：

```rust
if self.vad.is_empty() && !self.vad.detected() {
    self.vad.reset();
}
```

sherpa 的 `Detected()` 返回 `start_ != -1`，只有语音概率越过阈值后才置位；而 `Reset()` 会清空
Silero 的循环状态、环形缓冲和未成段数据。语音刚起始的若干窗口里 `detected()` 仍是 false，
此时 reset 把正在累积的状态抹掉 —— 于是 `detected()` 永远无法置位，reset 每块反复触发，
形成自锁：**VAD 再也切不出任何句段**。

实测 5.59s 清晰中文音频，带 reset 时 `detected()` 在全部 56 个块中**一次都没有为真**，产出 0 个句段；
去掉 reset 后立即切出 1 个 4.79s 句段，文本完全正确。

### 2. `accept()` 单次喂入过长音频（文件路径丢开头）

`recognize_file_segments` 按 10 秒切块调用 `accept()`，直接把整块交给 `vad.accept_waveform()`。
sherpa VAD 在语音确认前会裁剪缓冲，单次喂入越长，被裁掉的开头越多。同一段音频实测：

| 单次喂入样本数 | 时长 | 识别结果 |
| --- | --- | --- |
| 512 / 1024 / 1600 / 3200 / 4800 | ≤0.3s | `开放时间早上九点至下午五点`（完整） |
| 8000 | 0.5s | `时间早上九点至下午五点`（丢"开放"） |
| 16000 / 32000 | 1–2s | `早上九点至下午五点`（丢更多） |
| 160000 | 整段 | `嗯`（几乎全丢） |

实时路径因为麦克风本来就是小块送达，只受缺陷 1 影响；文件路径两个缺陷都中。

## 正确做法

- `accept()` 内部按 `vadWindowSize`（默认 512）切片喂入，调用方可以传任意长度，切片责任收敛在一处，
  实时与文件两条路径同时受保护。
- `drain()` 中**不要** reset。缓冲增长由 `recognize_file_segments` 的周期性 `flush_and_reset` 收口，
  sherpa 自身在非语音段也会裁剪缓冲，不存在无界增长。
- 上游所有 sherpa-onnx VAD 示例都是按 window_size 定长喂入，这是隐含契约而非优化建议。

## 测试教训

`recognizes_official_sensevoice_wave_and_vad_segment` 当时只断言"结果非空"，在只切出一个语气词
`嗯` 的情况下依然通过，完全掩盖了缺陷。而且它被 `#[ignore]` 标记、需要环境变量，从未真正跑过。

现已加固：

- 断言分段结果字数与整句识别相当，退化结果（如只剩 `嗯`）直接失败；
- 单独回归"小块喂入"的实时路径，缺陷 1 会让它一个句段都切不出来；
- `docs/rules/新增供应商与模型操作手册.md` 早已写明"涉及新引擎时必须跑真实模型音频夹具"，
  本次正是没有执行这条规则才让问题进入主干。

## 复现与验证

```bash
# 官方测试音频
curl -L -o test.wav "https://huggingface.co/csukuangfj/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-int8-2025-09-09/resolve/main/test_wavs/zh.wav"
cp test.wav "<数据根>/models/official.sherpa-sensevoice-small/"

cd src-tauri
SAYIT_SENSEVOICE_POC_DIR="<数据根>/models/official.sherpa-sensevoice-small" \
  cargo test --bins recognizes_official_sensevoice -- --ignored --nocapture
```

注意模型权重在数据根的 `models/` 下，数据根可能被 `data-root.json` 指针改到自定义位置，
不一定在 `%LOCALAPPDATA%\com.henjicc.sayit`。验证完记得删掉临时塞进模型目录的 `test.wav`。
