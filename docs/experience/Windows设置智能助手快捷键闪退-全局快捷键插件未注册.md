# Windows 设置智能助手快捷键闪退：全局快捷键插件未注册

## 现象

Windows 上给智能助手录入快捷键（如 Alt+Z）后应用立即退出，退出码 `0xc0000409`。应用日志和 Windows 事件日志都没有记录；panic 信息只打印在 `tauri:dev` 终端里：

```
state() called before manage() for tauri_plugin_global_shortcut::GlobalShortcut<...>
```

## 原因

听写与字幕快捷键在 Windows 上走 `hotkey.rs` 的低级键盘钩子，`tauri-plugin-global-shortcut` 原先只在非 Windows 平台注册；而 `application/assistant.rs` 在所有平台都用这个插件注册助手快捷键。`app.global_shortcut()` 找不到插件状态就 panic，panic 发生在 WebView2 回调里无法展开，进程直接中止。

## 做法

- 插件在所有平台注册（`main.rs` 的 `tauri::Builder`）。
- 任何代码调用某个插件的 `XxxExt` 扩展方法前，确认该插件在当前平台的 `Builder` 上已经 `.plugin(...)`；按平台条件编译注册插件时，搜一遍所有调用方。
- 开发版排查闪退时设 `RUST_BACKTRACE=1` 并把 `tauri:dev` 输出重定向到文件，再复现。
