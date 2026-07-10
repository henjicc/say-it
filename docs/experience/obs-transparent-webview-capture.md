# OBS 无法捕获透明字幕窗口

## 触发条件

Windows 10 上，OBS 使用窗口采集读取 Tauri/WebView2 的透明字幕悬浮窗时，预览可能只显示黑帧。

## 处理方式

在创建任何 WebView2 窗口之前，通过 `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS` 传入 `--disable-gpu`，让 WebView2 使用软件渲染，避免透明窗口的 GPU 合成内容无法被窗口采集链路读取。

## 验证

重启桌面应用后，打开实时字幕，在 OBS 的窗口采集中选择“说吧！”字幕窗口，确认字幕文本可见且会实时更新。
