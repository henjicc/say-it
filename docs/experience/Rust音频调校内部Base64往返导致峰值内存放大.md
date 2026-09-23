# Rust 音频调校内部 Base64 往返导致峰值内存放大

## 原因与边界

前端迁移为领域快照后，`process_offline` 的调用方已经是 Rust `AudioLabRuntime`，
但结果仍沿用旧的 `PCM → 字节 → Base64 → 字节 → PCM` 表示。
降噪、增益处理也各自分配整段音频，导致任务峰值显著高于最终需要保留的原声和处理声。

领域内部直接传递 `Vec<f32>` 所有权；波形和统计仍使用原有快照，音频不会传回前端。
降噪按原来的 480 样本帧处理，先复制一帧输入到独立数组再原地写回；尾帧仍补零，
干湿混合仍读取本帧原声。增益和限幅原地应用，保持浮点运算与算法顺序。

## 验证方法

先保留优化前的 release 测试程序及其 DLL，再构建优化后的测试程序：

```powershell
node scripts/run-rust-tests.mjs --release --no-run
node scripts/measure-audio-performance.mjs --executable <测试程序.exe> --output <结果.json>
node scripts/measure-audio-performance.mjs --executable <测试程序.exe> --output <降噪结果.json> --seconds 3 --denoise
```

测试程序路径以 Cargo 输出为准。Windows 中文目录构建的 NASM 问题见
[既有记录](本机release构建NASM不支持中文路径.md)。每次采样启动独立进程，使用
`PROCESS_MEMORY_COUNTERS_EX` 读取私有提交峰值，避免同一进程之前的任务污染峰值。
结果包含二进制 SHA-256、输入时长、降噪开关、输出校验值、统计和回收后内存。

2026-09-24，本机 Windows x64 release，300 秒 48 kHz 单声道合成音频，关闭降噪：

| 指标（三次采样中位数） | 优化前复测 | 优化后 |
| --- | ---: | ---: |
| 进程私有提交峰值 | 302.34 MiB | 120.70 MiB |
| 调校处理耗时（包含波形摘要） | 516.66 ms | 394.30 ms |
| 全量输出校验值 | `3ccc040ff5928c85` | `3ccc040ff5928c85` |

3 秒开启降噪的全量输出校验值也相同：`85cb3c851ff27b5c`。
回归覆盖空音频、静音、单样本、整帧和非整帧尾部、16/44.1/48 kHz 输入及原始素材保留。
这是独立音频链路的结果，不代表整机空闲内存降幅；原声和处理声仍随时长增长，
后续长音频流式化需要独立解决这一部分，不能据此宣称缓冲已经有界。
