# Symphonia 可解封装 Opus，但需单独接入解码器

## 现象

文件选择器允许 `.opus`、`.ogg`、`.webm`、`.mp4` 等格式。Symphonia 0.6 能从 Ogg、Matroska/WebM、
ISO BMFF/MP4 中识别并拆出 `CODEC_ID_OPUS` 音轨，但其默认 codec registry 没有 Opus 解码器。
因此只调用 `make_audio_decoder` 会在本地模型识别和同步短音频识别的预处理阶段失败；异步云端转写因上传原文件，不受此问题影响。

## 正确做法

- 继续由 Symphonia 负责容器探测、选轨和拆包，遇到 `CODEC_ID_OPUS` 时把 packet 交给 `audiopus` 解码。
- 按 Opus 固定的 48 kHz 解码，再复用统一的下混和 16 kHz 重采样流程。
- 应用 `OpusHead` 的 pre-skip 与 output gain，并优先遵守 Symphonia packet 上明确给出的首尾裁剪，避免编码延迟或尾部填充进入识别音频。
- Ogg/Matroska 的标准 `OpusHead` 多字节字段是小端序；MP4 的 `dOps` 是大端序。Symphonia 会为 `dOps` 补上 `OpusHead` 标记，但保留 `version=0` 和原字节序，解析时必须据此区分，否则 312 帧的 pre-skip 会被误读为 14337 帧。
- 普通 `audiopus::Decoder` 只支持单声道和双声道。遇到多声道 Opus 应明确报错，不能错误地按双声道解码。
- Symphonia 的默认解码器仍不覆盖 ALAC 和 MPEG Layer II。macOS 可在 Symphonia 失败后用 AudioToolbox `ExtAudioFile` 作为平台兜底，并直接请求 16 kHz、单声道、Float32 客户端格式，让系统完成解码、下混和重采样；这样 ALAC-in-M4A 与 MP2-in-MPEG 也能进入本地模型和同步短音频通路。WMA/WMV、AVI、FLV 不属于 AudioToolbox 可打开的音频文件，仍只能走上传原文件的异步云端通路，不能把原生兜底描述成全格式解码器。

## 验证

- 单元测试生成标准 Ogg Opus，验证解封装、解码、下混与 16 kHz 输出。
- 使用 ffmpeg 生成真实的 Opus-in-WebM 和 Opus-in-MP4 文件，验证两种容器均能输出非静音 PCM，且时长约等于输入。
- macOS 测试用系统自带 `afconvert` 生成 ALAC-in-M4A，锁定 Symphonia 失败后 AudioToolbox 兜底能输出约 16,000 个非静音样本。
