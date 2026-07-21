# 枚举窗口取进程名时 UWP 应用都指向 ApplicationFrameHost

## 问题

按进程名区分软件（黑名单、按软件配置规则等）时，凡是 UWP / 打包应用——设置、
新版记事本、计算器、部分商店应用——取到的进程名全是 `ApplicationFrameHost.exe`。
后果是这些应用在功能上无法区分：给记事本配的规则会同时命中"设置"，黑名单拦一个
就等于拦一片。

## 原因

UWP 应用的顶层窗口（类名 `ApplicationFrameWindow`）由系统的框架宿主进程托管，
`GetWindowThreadProcessId` 拿到的是宿主 PID，不是应用自己的 PID。真实应用进程挂在
子窗口上，类名为 `Windows.UI.Core.CoreWindow`。

## 做法

取到进程名后判断是否为框架宿主，是则 `EnumChildWindows` 找 `CoreWindow` 子窗口，
取它的 PID 作为真实进程：

```rust
fn resolve_real_process(window: HWND, process_id: u32) -> u32 {
    let host = process_name(process_id).unwrap_or_default().to_lowercase();
    if host != "applicationframehost.exe" {
        return process_id;
    }
    let mut found: Option<u32> = None;
    let _ = unsafe { EnumChildWindows(window, Some(collect_core_window), /* &mut found */) };
    found.filter(|pid| *pid != process_id).unwrap_or(process_id)
}
```

实现见 `src-tauri/src/active_app_context/windows.rs`。

要点：

- 子窗口可能仍属于宿主进程（应用正在启动、或被系统合并），这种情况下没有更好的
  答案，必须保持原 PID 而不是返回失败——调用方拿到宿主名总好过拿到空值。
- 前台窗口路径（`GetForegroundWindow`）和窗口枚举路径（`EnumWindows`）都要过这层
  解析，否则"从列表里选的软件"和"听写时识别到的软件"对不上，规则永远不命中。

## 顺带的枚举过滤条件

`EnumWindows` 想得到"用户能切换过去的软件"，四个条件缺一不可：`IsWindowVisible`、
非 `WS_EX_TOOLWINDOW`、`GetWindow(GW_OWNER)` 为空（排除对话框等附属窗口）、
窗口标题非空。少任何一条，列表里都会混进大量不可见的系统窗口。
