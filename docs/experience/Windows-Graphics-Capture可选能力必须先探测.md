# Windows Graphics Capture 可选能力必须先探测

## 触发条件

使用 `windows-capture` 设置 `WithoutBorder`、`WithoutCursor` 或 `Exclude` 时，部分 Windows 版本虽然支持窗口捕获，但不支持修改这些会话属性，捕获会在首帧前直接失败。

## 正确做法

- 分别调用 `GraphicsCaptureApi::is_border_settings_supported`、`is_cursor_settings_supported` 和 `is_secondary_windows_supported`。
- 不支持或探测失败时使用对应的 `Default`，不要让可选能力阻断截图。
- 即使探测结果为支持，增强设置初始化失败时仍应使用全套 `Default` 自动重试一次。
- 将兼容降级写入调试诊断，但不要把它当成捕获失败。

该问题与 Tauri 的 dev/release 启动方式无关，取决于 Windows Graphics Capture API 契约版本。
