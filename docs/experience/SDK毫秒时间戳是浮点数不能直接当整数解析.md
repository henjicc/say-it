# SDK 毫秒时间戳是浮点数，不能直接当整数解析

## 现象

在「说吧」里用 Groq ASR 模型做语音输入，识别完成后报错：

```
语音输入出错：内置 SDK 识别结果映射失败：invalid type: floating point `25049.99936`, expected u64
```

音频越长越容易触发，但和时长本身无关——只要供应商返回的秒数不是整秒就会命中。

## 根因

`@henjicc/ai-sdk` 的 `SpeechRecognitionOutput` 里 `durationMs` / `startMs` / `endMs` 的类型是
`number`，**不保证是整数**。Groq 的响应用秒计时（`duration: 25.04999936`），SDK 内部统一
`seconds * 1000` 换算成毫秒，于是产出小数毫秒 `25049.99936`。

而 say-it 侧 `TranscriptionResult` 的契约是整数毫秒（`duration_ms: Option<u64>`、
`begin_time: u64`）。`src-tauri/src/providers/sdk_runtime/online.rs` 的 `sdk_asr_to_legacy`
原来这么写：

- `durationMs` 直接 `cloned()` 原样透传 → serde 反序列化 `f64` 到 `u64` 直接报错，整次识别失败。
- 句子/单词的 `startMs` / `endMs` 用 `Value::as_u64()` → 遇到浮点返回 `None`，被
  `unwrap_or_default()` 吞成 `0`，**静默丢失所有时间戳**，比报错更隐蔽。

这是宿主侧的序列化问题，不是 SDK 的契约违背，按项目规则应在 say-it 修，不动 SDK。

## 修复

`online.rs` 增加 `sdk_millis(Option<&Value>) -> Option<u64>`，统一处理 u64 / i64 / f64
三种 JSON 数字：四舍五入取整，负值钳到 0，非有限值返回 `None`。`durationMs`、句子和单词的
`startMs` / `endMs` 全部改走这个函数。

## 通用结论

跨 SDK 边界读取任何时间量时：

- **不要假设「毫秒」等于「整数」**。凡是供应商按秒计时、SDK 做过单位换算的字段，都可能是小数。
- **不要用 `Value::as_u64()` 读可能是浮点的数字**，它对 `1234.5` 返回 `None` 而不是报错，
  配合 `unwrap_or_default()` 会变成静默的 0，排查成本远高于直接失败。
- 新接入供应商时，重点检查响应里所有时间/时长字段的单位与精度，而不是只看字段名对不对。
