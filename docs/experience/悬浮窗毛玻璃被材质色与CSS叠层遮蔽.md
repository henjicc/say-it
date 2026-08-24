# 悬浮窗毛玻璃被材质色与 CSS 叠层遮蔽

## 现象

透明窗口开启系统毛玻璃后，界面像覆盖了一层灰白色蒙版，看不到背景被模糊的质感。

## 原因

- macOS 的 `HudWindow` 材质自带明显的固定灰色外观，不适合需要保留桌面背景的悬浮窗。
- WebView 内容层又使用了较高不透明度的背景色，进一步遮住原生模糊层。
- Windows Acrylic 的 tint alpha 过高也会产生相同的“纯色蒙版”观感。

## 正确做法

- macOS 使用 `UnderWindowBackground`，并让窗口保持透明、使用深色主题。
- Windows Acrylic 只保留低透明度深色 tint。
- WebView 的玻璃状态只叠加很淡的颜色层，不使用高不透明度背景、渐变或发光。
- 不使用 CSS `backdrop-filter` 冒充系统级毛玻璃；它无法可靠采样 WebView 窗口之外的桌面内容。
