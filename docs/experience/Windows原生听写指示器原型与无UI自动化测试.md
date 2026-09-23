# Windows 原生听写指示器：窗口陷阱与无 UI 自动化测试方法

分支 `feat/native-indicator-windows` 的实践经验，适用于后续悬浮球等其他小窗的原生化。

## 为什么做

每个 Tauri 窗口 = 一个 WebView2 渲染进程，闲时约 60MB 工作集。听写指示器在
`main.rs` 启动时无条件预创建（`ensure_indicator_window`），即使用户从不听写也常驻。
原生化的实测对比（主窗口 + 指示器两个场景）：

| 场景 | 进程数 | 工作集 |
|---|---|---|
| WebView 指示器常驻（隐藏） | 9 | ~562 MB |
| 原生指示器（无该 WebView） | 8 | ~448 MB（省 ~114 MB，-20%） |

私有字节（PrivateBytes）受 WebView2 GPU 进程 GC 影响波动极大（127~320MB 同一条件
反复横跳），对比时以工作集（WorkingSet）为准。

## Win32 原生窗口的三个坑（都踩过）

1. **`PostThreadMessageW` 的线程消息不进窗口过程。** 跨线程通知窗口"命令队列有货"
   时，如果用 `PostThreadMessageW(thread_id, ...)`，`GetMessageW` 取到的是
   `hwnd = NULL` 的线程消息，`DispatchMessageW` 不会调用任何窗口过程——命令永远
   堆积。要么在消息循环里直接认领（`msg.hwnd` 为空时自行处理），要么改用
   `PostMessageW(hwnd, ...)`。
2. **`UpdateLayeredWindow` 不会显示从未显示过的窗口。** 创建时没带 `WS_VISIBLE`
   的分层窗口，光调 `UpdateLayeredWindow` 上屏内容是不够的，窗口仍不可见；首次
   渲染后必须显式 `ShowWindow(SW_SHOWNOACTIVATE)` 一次。
3. **GUI 子系统下 `eprintln!` 无处可去。** 用 `Start-Process` 拉起调试 exe 时
   stdout/stderr 都是死的，窗口创建失败完全无声。排障时直接写临时文件最快。

## 无 UI 自动化测试听写链路的方法

听写热键走的是低级键盘钩子（`hotkey.rs`），它**刻意忽略 `LLKHF_INJECTED` 注入事件**
（防 enigo 模拟按键误触发），所以 keybd_event/SendInput 永远无法触发听写。可行路径：

- 启动时设 `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9223`
  （注意 main.rs 曾经直接覆盖这个变量，已改为追加），然后 CDP 连接
  `http://localhost:9223/json`，在主窗口页面里
  `window.__TAURI__.core.invoke('set_indicator_state', {state:'recording'})`
  即可驱动指示器全状态机（recording/processing/fallback/hidden + set_indicator_text）。
- **不要在 CDP 同步 invoke 里触发会创建 WebView 窗口的命令**（如 `error` 态回退
  路径）：`set_indicator_state` 不是 async command，Windows 上在 WebView2 同步
  IPC 回调里建窗会重入死锁，整个渲染进程卡死。真实听写流程从 Rust 侧调用，无此问题。
- 原生窗口验证：EnumWindows 按 PID + 类名（`SayItNativeDictationIndicator`）确认
  存在性与可见性；屏幕截图用 `Graphics.CopyFromScreen` 截主屏底部居中区域。
- 内存测量脚本 `scripts/测量进程内存.ps1` 是 UTF-8 无 BOM，必须用 **pwsh 7**
  运行；Windows PowerShell 5.1 会按 ANSI 解析直接报语法错误。

## 原生窗口的渲染目标必须用软件模式

`D2D1_RENDER_TARGET_TYPE_DEFAULT` 会创建 D3D11 硬件设备，把整套显卡驱动用户态 DLL
拉进进程（NVIDIA 机器上是 nvgpucomp64 + nvwgf2umx，映射 ~180MB，驻留 ~26MB）。
悬浮球/指示器这种几十 KB 像素的窗口毫无 GPU 加速需求，改
`D2D1_RENDER_TARGET_TYPE_SOFTWARE`（WARP 软光栅）后视觉无差别，托盘驻留从
90MB 降到 64MB。排查手段：`Get-Process | Select -Expand Modules` 按映射大小排序，
GPU 驱动 DLL 一目了然。

## 隐藏后再显示：先 ShowWindow 再 UpdateLayeredWindow

对 `SW_HIDE` 过的分层窗口直接 `UpdateLayeredWindow` 可能失败；若把 `ShowWindow`
放在 ULW 之后，失败后 `shown` 标志没置位，之后每帧都重复失败，窗口永远回不来。
`LayeredSurface::render` 里固定顺序：先 `ShowWindow(SW_SHOWNOACTIVATE)`（仅首次），
再 ULW 提交内容。

## 屏幕截图验证的坐标系陷阱

PowerShell 的 `CopyFromScreen` 坐标空间随调用进程的 DPI 感知上下文变化（同一台
双 4K@150% 机器上，不同 pwsh 进程分别报告过 3840×2160 和 2560×1440），按它算
截图区域会截错地方，看起来像"窗口没渲染"。**用 Python PIL `ImageGrab.grab`
（物理像素）截屏**，窗口矩形用 ctypes EnumWindows + GetWindowRect 拿，两者坐标系
一致。pwsh 的 EnumWindows 探测还会在委托回调里吞掉 Write-Output（被外层
`| Out-Null` 连管道吃掉），探测脚本输出要用数组收集后统一打印。

## 交互型原生窗口（悬浮球）的另外两个坑

4. **窗口类光标必须显式指定。** `WNDCLASSW` 的 `hCursor` 为 NULL 时，悬停光标沿用进入
   窗口前的状态——常常停在"后台忙碌"转圈上，看起来像卡死。注册类时
   `LoadCursorW(None, IDC_ARROW)` 即可。
5. **激活类点击要在当帧同步切点击穿透。** 悬浮球的 activate 流程会转发一次模拟点击
   到球所在位置来恢复焦点（`floating_orb.rs` 的 forward_click），这要求球已经处于
   `WS_EX_TRANSPARENT` 穿透态。原生窗口的样式切换若只走异步命令队列，转发点击可能
   抢在切换前落回球上被吃掉 → 焦点回不来 → 文本粘贴不上。点击被接受的当帧就在
   窗口过程里同步切换，激活失败时再恢复。

## 原型已知行为差异（后续若要转正需补齐）

- 无入场/淡出动画、无波形柱缓动（直接跳变）、无阴影/背景模糊；
- 文本 `fade`（swapText）语义未实现，一律直接替换；
- `set_indicator_layout` 对原生窗口是 no-op（几何固定 460×188 主屏底部居中）；
- error 态回退 WebView 后，该 WebView 进程本 session 内常驻（下次听写才隐藏不销毁）。
