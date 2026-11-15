# 原生回答窗：macOS 真机检查

macOS 后端只在 Windows 上做过类型检查（`npm run check:mac-overlay`），运行效果要在 Mac 上逐项确认。先跑一遍 [01-Windows回归检查](01-Windows回归检查.md) 里的全部条目，再看下面这些 Mac 特有的项。

## 窗口与焦点

- [ ] CI 的 `verify (macos-15)` 里 overlay 测试通过（含 `overlay_classes_register_with_valid_appkit_signatures`，它会校验 NSTextInputClient 方法签名）。
- [ ] 回答窗出现后直接打字能进输入框，说吧本身没有被激活（菜单栏仍是原来的应用）；点回答窗里任意位置后也能打字。
- [ ] 回答窗失去焦点（点别的应用）且没置顶时关闭；置顶后不关闭。
- [ ] 指针与工作区坐标在 Retina 屏和外接非 Retina 屏上都对得上：回答窗左上角对齐鼠标，不跑到别的屏或偏移一段距离（`native_answer_show` 把 `cursor_position` 和 `work_area` 都当作 DesktopPx）。

## 文字

- [ ] 加粗、斜体、行内代码（Menlo）、链接下划线正常；删除线在 Mac 上不画（CoreText 没有对应属性）。
- [ ] 拖选、双击选词、三击选段的高亮与文字对齐；跨行选区在行尾留出一点宽度。
- [ ] 链接与行内代码的颜色分段正确，相邻两段颜色不会串。

## 键盘与输入法

- [ ] 拼音输入：组字串带下划线显示在输入框光标处，候选窗贴在光标下方；回车上屏、Esc 取消组字，组字时方向键与回车由输入法处理。
- [ ] Command+A/C/X/V 可用；回车发送、Shift+回车换行；Esc 关闭或放弃语音追问。
- [ ] 触控板与鼠标滚轮都能滚动正文，触控板滚动速度自然（一行约 20 点）。
