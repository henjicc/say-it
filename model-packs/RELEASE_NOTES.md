## 本地模型包（离线识别 / OCR）

模型包只发布一次，链接长期有效，**不随应用版本更新**。已经装过的用户升级应用后无需重新下载。

| 模型包 | 用途 | 大小 | 下载 |
| --- | --- | --- | --- |
| SenseVoice Small INT8 | 本地语音识别（整句听写、录音转写，多语种） | 227 MB | [下载](https://github.com/henjicc/say-it/releases/download/models-v1/official.sherpa-sensevoice-small-1.0.0-embedded.sayit) |
| Paraformer Online INT8 | 本地语音识别（实时流式，中英双语） | 226 MB | [下载](https://github.com/henjicc/say-it/releases/download/models-v1/official.sherpa-paraformer-online-1.0.0-embedded.sayit) |
| PP-OCRv6 Tiny | 本地 OCR（屏幕取词、上下文识别） | 3 MB | [下载](https://github.com/henjicc/say-it/releases/download/models-v1/official.ppocr-v6-tiny-1.0.0-embedded.sayit) |

**安装方式**：下载后双击 `.sayit` 文件，或在「设置 → 插件管理 → 安装 .sayit 包」中选择。首次安装会提示确认签名密钥（`henjicc-sayit-publisher-v1`），确认后即可在模型下拉中启用。

模型包内嵌完整权重，安装后**完全离线运行**，不需要联网也不消耗 API 额度。
