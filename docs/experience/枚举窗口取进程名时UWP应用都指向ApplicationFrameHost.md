# 枚举窗口取进程名时 UWP 应用都指向 ApplicationFrameHost

## 问题

按进程名区分软件（黑名单、软件规则等）时，凡是 UWP / 打包应用——设置、新版记事本、
计算器、部分商店应用——取到的进程名全是 `ApplicationFrameHost.exe`。后果是这些应用
在功能上无法区分：给记事本配的规则会同时命中"设置"，黑名单拦一个等于拦一片。

## 原因

UWP 应用的顶层窗口（类名 `ApplicationFrameWindow`）由系统的框架宿主进程托管，
`GetWindowThreadProcessId` 拿到的是宿主 PID，不是应用自己的 PID。

## 两个流传很广但实测无效的做法

在 Windows 10 19045 上实测，以下两种网上常见做法都拿不到真实进程：

- **`EnumChildWindows` 找 `Windows.UI.Core.CoreWindow` 子窗口**：框架窗口的子树里
  只有 `ApplicationFrameTitleBarWindow` 和 `ApplicationFrameInputSinkWindow`，
  **全部属于宿主进程**，没有 CoreWindow。
- **`GetPropW(hwnd, "ApplicationViewCoreWindow")`**：返回空句柄。

真实情况是：`CoreWindow` 是**独立的顶层窗口**（`EnumWindows` 能直接枚举到，PID 正确），
它和框架窗口之间没有父子关系，也没有可用的窗口属性关联。

## 可行做法：按标题关联

框架窗口和它承载的 CoreWindow 标题一致，这是仅剩的可用线索：

```rust
fn resolve_real_process(window: HWND, process_id: u32) -> Option<u32> {
    // 先按类名判断，非 UWP 窗口不做任何跨进程调用
    if class_name(window) != "ApplicationFrameWindow" {
        return Some(process_id);
    }
    // 枚举顶层窗口，找标题相同、PID 不同于宿主的 CoreWindow
    resolve_uwp_process(window, process_id)
}
```

实现见 `src-tauri/src/active_app_context/windows.rs`。

要点：

- **返回 `Option`，不要回落到宿主 PID**。列表枚举场景下关联失败就丢弃该窗口：应用会
  以自己的 CoreWindow 顶层窗口另行出现，留着框架窗口只会多一条认不出的重复条目
  （实测未丢弃时，"设置"会同时以 `ApplicationFrameHost` 和 `SystemSettings` 出现两次）。
  上下文捕获场景相反，可以 `unwrap_or(宿主 PID)` 保底，因为窗口元信息本身就有价值。
- **所有取进程名的路径都要过这层解析**——前台窗口（`GetForegroundWindow`）、窗口枚举
  （`EnumWindows`）、上下文基线（`baseline_context`）缺一不可，否则"列表里选的软件"和
  "听写时识别到的软件"对不上，规则永远不命中。

## 枚举顶层窗口的过滤条件与开销

想得到"用户能切换过去的软件"，四个条件缺一不可：`IsWindowVisible`、非 `WS_EX_TOOLWINDOW`、
`GetWindow(GW_OWNER)` 为空（排除对话框等附属窗口）、窗口标题非空。少任何一条，列表里都会
混进大量不可见的系统窗口。

性能上把过滤按代价从低到高排：窗口样式和标题这些本进程可直接读的属性先判，
**按 PID 去重之后再 `OpenProcess` 取进程名**。同一进程常开多个窗口（浏览器、资源管理器），
不去重会重复开几十次句柄。实测全量枚举约 2–4ms。

注意 UWP 解析必须排在 PID 去重**之前**，否则多个 UWP 应用会因为共用宿主 PID 被折叠成一个。
