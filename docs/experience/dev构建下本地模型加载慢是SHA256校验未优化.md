# dev 构建下本地模型加载慢是 SHA256 校验未优化

## 现象

用 `npm run tauri:dev` 运行时，本地语音模型（SenseVoice、Fun-ASR 等）每次开始听写都要等 6–7 秒才出字；release 构建只要 1–2 秒。容易误判为模型或 sherpa-onnx 运行时慢。

## 根因

本地模型每次创建识别器前都会经 `providers/model_download.rs` 对全部文件做完整 SHA256 校验。`[profile.dev]` 没有 opt-level，`sha2` 按 opt-level 0 编译，263 MB 的 Fun-ASR 模型校验约 5.3 秒；sherpa-onnx/ONNX Runtime 是预编译的 Release DLL，不受 dev profile 影响。同理，RNNoise 降噪（nnnoiseless）、响度（ebur128）与 OCR 的 Rust 侧预处理（ocr-rs、image、png、fast_image_resize）在 dev 下也慢 3–20 倍。

## 正确做法

- 在 `src-tauri/Cargo.toml` 用 `[profile.dev.package.<crate>] opt-level = 3` 只给这些热点依赖开优化（主包与 release 不变）。实测 dev 下：校验 263 MB 5.3 s → 0.16 s；120 秒音频降噪 14.3 s → 0.9 s；OCR 三图中位 2375 ms → 475 ms；主包增量编译不变慢。
- 判断本地识别/OCR 是否“慢”时，先确认是 dev 还是 release 构建；性能结论以 release 为准。
- 新增大文件哈希、图像或音频 DSP 依赖时，评估是否需要加入这份 dev 优化清单，不要给主包或全部依赖（`"*"`）开优化。
