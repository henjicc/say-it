# Alt 系统快捷键让前台程序进入菜单，导致粘贴无效

## 现象

Windows 上用 Alt+Z 触发智能助手「翻译输入」：历史记录里有译文，诊断日志显示 `dictation.injectionFinished` 成功，但输入框里什么都没有；按快捷键时目标程序（如 Claude 桌面版）左上角弹出菜单。

## 原因

助手快捷键经 `tauri-plugin-global-shortcut` 用 `RegisterHotKey` 注册。系统吞掉主键 Z，前台程序只收到 Alt 被单独按下又松开，于是进入菜单栏模式；之后模拟的 Ctrl+V 被菜单吃掉。按键发送本身不报错，所以注入被判为成功。

`clipboard.pasteReadTimeout` 不能作为粘贴失败的判据：Chromium 系程序在按粘贴键前就会读走剪贴板，正常粘贴也会记这条。

## 做法

- 含 Alt 或 Win 的系统快捷键按下时，趁修饰键还按着补发一个未分配的虚拟键（`0xE8`，`hotkey::mask_modifier_release`），松开就不再算单独按下。
- 结果本该直接上屏的任务（听写、翻译输入、可编辑选区改写）注入失败时，统一留在剪贴板并显示「已复制，请粘贴」。
