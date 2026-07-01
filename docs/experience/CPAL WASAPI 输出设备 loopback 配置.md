# CPAL WASAPI 输出设备 loopback 配置

## 触发条件

在 Windows 上用 CPAL 采集系统音频时，会把播放设备作为 input stream 打开，让 WASAPI 进入 loopback 模式。

如果对播放设备调用 `default_input_config()`，会失败并返回：

```text
The requested stream type is not supported by the device.
```

## 正确做法

播放设备本身是 output device，应先用 `default_output_config()` 读取混音格式，再用同一个播放设备创建 input stream。

CPAL 的 WASAPI 后端会在 `build_input_stream` 遇到 render device 时自动加上 loopback 标记。
