# 文件解码整段 PCM 缓冲放大内存与分块重采样边界

## 触发场景与根因

本地文件识别原先先把全部解码包下混、追加到原采样率 `Vec<f32>`，再分配整段
16kHz PCM，最后才开始 VAD 与识别。长文件既保留多份音频，还承受 Vec 扩容的临时峰值。
这部分占用与 Tauri 窗口、WebView、识别模型无关，销毁窗口不会解决。

## 实现约束

- `audio_prep::decode_mono_16k_chunks` 同步解码、重采样和消费，用消费速度限制生产速度。
  Windows/Linux 不再缓存整段 PCM，不引入额外文件或无界队列。
- 重采样必须保留全局样本位置，运算顺序与原 `resample_linear` 一致。不能对每个包独立
  重采样后拼接，否则舍入、插值和尾部会随包边界变化。
- 下采样时，只有累计样本数能保证该输出存在后才可提交。高倍率下仅保留前包最后一个
  样本不够：首个输出可能需要等待若干输入样本，必须保留下一插值位置之后的数据。
- 本地文件 VAD 仍按十秒块喂入、每六块收口。内部继续按 `vadWindowSize` 喂给 sherpa；
  解码包边界不能成为识别或 reset 边界。该缓冲最多 160,000 个 f32，约 625 KiB。
- 每包检查取消；消费失败立即向上传播，任务失败或取消不提交部分结果。单次原生模型
  初始化/推理不能被这个检查抢占，要等其返回后才能响应取消。
- macOS 保留已有整段输出和原生回退语义。主解码器中途失败后直接向同一个消费者重放
  原生输出会导致重复识别，不能为了流式化引入该错误；macOS 的完整分块回退仍待优化。

## 发布模式实测

2026-09-24，Windows x64，同机、同构建参数，各三个独立 release 测试进程的中位数。
夹具为本地生成的 48kHz 双声道 PCM16 WAV。计时包括解码和输出散列，不包括夹具生成。
旧实现收集后消费，新实现分块消费；消费同样的全部样本，散列一致。

| 音频时长 | 原进程私有提交峰值 | 分块后峰值 | 原耗时 | 分块后耗时 | 输出散列 |
| --- | ---: | ---: | ---: | ---: | --- |
| 5 分钟 | 100.98 MiB | 3.82 MiB | 150.46 ms | 110.57 ms | `7ea74e6e9e82f725` |
| 30 分钟 | 774.29 MiB | 3.82 MiB | 938.86 ms | 682.79 ms | `3c7baa6a829d9325` |

这些是隔离解码进程数据，**不包括 WebView、模型初始化或识别推理，不能当作应用总内存**。
30 分钟 PCM 共 28,800,000 个样本，分块不截断、不降质，也没有把 PCM 转存到磁盘。
仍调用 `decode_to_mono_16k` 的模型对比会保留整段 16k 输出，尚未完全消除随时长增长的占用。

优化前基于 `9229842`，仅添加测量夹具与缓冲文件写入，未改变生产解码实现。
该基线测试程序 SHA256 为 `15dd3705d3f0fd0433e90d4888cc4abd9f2880de321bf0115eef15eb95828e01`。
本次测量的分块测试程序 SHA256 为 `ea3241018ec57dc0cb7da08690cd932b208d6bf63f4a7a4aa44d47eae1aaf2cc`。
本机对照二进制与原始 JSON 保留在 gitignore 的 `工作区/performance/`。

## 复现入口

```powershell
node scripts/run-rust-tests.mjs --release --no-run
node scripts/measure-audio-performance.mjs --scenario decode --seconds 1800 `
  --executable '<测试程序>' --output '<结果.json>'

$env:SAYIT_SENSEVOICE_POC_DIR = '<包含官方模型及 test.wav 的测试目录>'
node scripts/run-rust-tests.mjs --release streamed_sensevoice_matches_offline_text_and_timestamps -- --ignored --nocapture
```

重采样回归覆盖 8k～192kHz、单样本包、长短包和文件尾部，与原整段算法逐位比较。
识别回归使用官方本地 SenseVoice 模型，对短句、整十秒、整一分钟及跨分钟尾块分别核对
完整文本、每句起止时间与文件时长，不调用在线识别服务。
