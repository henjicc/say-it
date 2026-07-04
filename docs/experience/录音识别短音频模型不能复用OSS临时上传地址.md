# 录音识别短音频模型（fun-asr-flash / qwen3-asr-flash）不能复用 OSS 临时上传地址

## 现象

录音识别功能提交短音频（同步识别路径）时报错：

```
提交短音频识别返回 400 Bad Request：Download failed with exit code: 1
```

而异步识别路径（fun-asr / paraformer / qwen3-asr-flash-filetrans）一切正常。

## 根因

`upload_for_model`（`src-tauri/src/providers/alibabacloud/uploads.rs`）通过 `getPolicy` 接口获取临时凭证并把本地文件上传到阿里云百炼的临时 OSS，返回值是形如 `oss://{key}` 的**私有资源地址**。

这个 `oss://` 地址只有异步转写接口
（`POST /api/v1/services/audio/asr/transcription`，配合请求头 `X-DashScope-OssResourceResolve: enable`）能够解析。

同步短音频识别走的是另一个接口
（`POST /api/v1/services/aigc/multimodal-generation/generation`，用于 `fun-asr-flash-2026-06-15` 和 `qwen3-asr-flash`），
它不认识 `oss://` 前缀，只接受：

- 公网可直接访问的 HTTP(S) URL；
- `data:{MIME_TYPE};base64,{DATA}` 格式的 Data URI。

之前的实现把 `oss://` 地址原样传入 `input_audio.data` / `content[].audio` 字段，服务端把它当成一个（不存在的）URL 去下载，于是返回 `Download failed`。

## 修复

- `recognize_short_audio`（`src-tauri/src/providers/alibabacloud/transcription.rs`）不再接收/使用 OSS 上传返回的 `file_url`，而是直接读取本地文件、Base64 编码后拼成 Data URI 传给同步接口。
- `run_transcription_job`（`src-tauri/src/commands/transcription.rs`）只在 `uses_async_transcription_task(&model)` 为真（fun-asr / paraformer / qwen3-asr-flash-filetrans）时才调用 `upload_for_model` 走 OSS 上传；同步短音频模型完全跳过 OSS 上传步骤。

## 参考

- `docs/API/阿里云百炼/非实时语音识别.md`：Fun-ASR-Flash / Qwen3-ASR-Flash 的 Base64 Data URI 输入方式。
- `docs/API/阿里云百炼/Fun-ASR录音文件识别HTTP API参考.md` 第 72 行：说明 `oss://` 临时 URL 仅用于异步 `file_urls` 接口，且需要 `X-DashScope-OssResourceResolve` 才能被动态解析（不推荐）。

## 适用场景

以后如果接入新的“同步”语音识别模型（走 multimodal-generation 或类似一次性返回结果的接口），本地文件都应直接编码为 Data URI 传输，不要复用 `upload_for_model` 返回的 OSS 临时地址。
