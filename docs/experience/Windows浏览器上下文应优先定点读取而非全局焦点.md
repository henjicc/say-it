# Windows 浏览器上下文应优先定点读取而非全局焦点

全局快捷键通常紧跟用户点击目标输入区触发。此时 `GetCursorPos` 仍能提供该输入区的屏幕坐标，而 Chromium/Electron 的 `IUIAutomation::GetFocusedElement` 可能经过多进程辅助功能桥，偶发占满原生文本读取预算。

原生上下文探针应先验证鼠标点仍属于激活窗口，再按顺序尝试：

1. `AccessibleObjectFromPoint`，直接读取该点的 MSAA/IA2 对象；
2. `IUIAutomation::ElementFromPoint`，读取 IA2 或 TextPattern；
3. 只有定点读取内容不足时才调用全局焦点查询并走原有祖先读取。

IA2 定点对象没有插入光标时，可用 `IAccessibleText::get_offsetAtPoint` 补充点击位置附近的一行文本。定点 MSAA 对象必须先检查 `STATE_SYSTEM_PROTECTED`，UIA 对象必须先检查 `IsPassword`；命中后不得读取正文或执行剪贴板兜底。
