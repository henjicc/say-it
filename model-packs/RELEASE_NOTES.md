## 本地模型包（离线识别 / 听写整理 / OCR）

模型包只发布一次，链接长期有效，**不随应用版本更新**。已经装过的用户升级应用后无需重新下载。

| 模型包 | 用途 | 大小 | 下载 |
| --- | --- | --- | --- |
| SenseVoice Small INT8 | 本地语音识别（整句听写、录音转写，多语种） | 227 MB | [下载](https://modelscope.cn/models/henjicc/sayit-models/resolve/master/official.sherpa-sensevoice-small/official.sherpa-sensevoice-small-1.0.2-embedded.sayit) |
| Fun-ASR-Nano CTC INT8 | 本地语音识别（整句听写，中文效果好） | 253 MB | [下载](https://modelscope.cn/models/henjicc/sayit-models/resolve/master/official.sherpa-funasr-nano-ctc/official.sherpa-funasr-nano-ctc-1.0.2-embedded.sayit) |
| Paraformer Online INT8 | 本地语音识别（实时流式，中英双语） | 226 MB | [下载](https://modelscope.cn/models/henjicc/sayit-models/resolve/master/official.sherpa-paraformer-online/official.sherpa-paraformer-online-1.0.1-embedded.sayit) |
| Qwen3-ASR-0.6B | 本地语音识别（整句听写、录音转写，自带标点，有显卡时更快） | 616 MB | [下载](https://modelscope.cn/models/henjicc/sayit-models/resolve/master/official.transcribe-qwen3-asr-0.6b/official.transcribe-qwen3-asr-0.6b-1.0.0-embedded.sayit) |
| Qwen3-ASR-1.7B | 本地语音识别（整句听写、录音转写，自带标点，准确率更高，建议有显卡） | 1.41 GB | [下载](https://modelscope.cn/models/henjicc/sayit-models/resolve/master/official.transcribe-qwen3-asr-1.7b/official.transcribe-qwen3-asr-1.7b-1.0.0-embedded.sayit) |
| Qwen3.5-4B 听写微调 | 本地听写整理（把口语整理成通顺的文字，建议有显卡） | 2.52 GB | [下载](https://modelscope.cn/models/henjicc/sayit-models/resolve/master/official.llama-polish-4b/official.llama-polish-4b-1.0.2-embedded.sayit) |
| CT-Transformer 中英标点 INT8 | 给本地语音识别的结果自动加标点 | 72 MB | [下载](https://modelscope.cn/models/henjicc/sayit-models/resolve/master/official.sherpa-ct-punct-zh-en/official.sherpa-ct-punct-zh-en-1.0.1-embedded.sayit) |
| PP-OCRv6 Tiny | 本地 OCR（屏幕取词、上下文识别） | 3 MB | 已随应用内置，无需安装模型包 |

**安装方式**：在「模型 → 插件管理 → 官方模型」里直接点「下载」；也可以下载上表的 `.sayit` 文件后双击，或在「插件管理 → 安装 .sayit 包」中选择。装好后即可在模型下拉中选用；标点模型包不进下拉，装好后本地识别结果自动加标点。

模型包内嵌完整权重，安装后**完全离线运行**，不需要联网也不消耗 API 额度。
