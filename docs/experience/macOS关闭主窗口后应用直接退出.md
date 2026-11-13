# macOS 关闭主窗口后应用直接退出，没有留在菜单栏

## 现象

macOS 上点主窗口的关闭按钮，应用整个退出，菜单栏图标也消失；没有崩溃报告。Windows 正常驻留托盘。

## 根因

- 主窗口关闭即销毁。听写胶囊、悬浮球、字幕条改成原生窗口后，它们不是 Tauri 窗口，关掉主窗口时 Tauri 里一个窗口都不剩。
- 最后一个窗口销毁时，tauri-runtime-wry 发出 `RunEvent::ExitRequested { code: None }`，不阻止就退出事件循环。
- `main.rs` 里 Windows 分支的 `app.run` 阻止了它，macOS 分支的 `app.run_return` 只处理了 `Reopen`、`Opened`，没有处理退出事件。以前 macOS 上还有 WebView 悬浮窗常驻，这个缺口没暴露。

## 做法

两个平台共用 `handle_exit_events`：`code: None` 时 `prevent_exit()`，`Exit` 时释放本地整理模型进程。托盘「退出」、`app.restart()` 带退出码，macOS 的 Cmd+Q 走 `applicationWillTerminate`，都不受影响。

## 以后

两个平台的事件循环分开写时，与平台无关的事件处理放进共用函数，不要只写在一边。
