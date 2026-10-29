# UpdateLayeredWindow 改变窗口位置不发 WM_MOVE：字幕入场位移不会漂移

## 疑点

原生字幕条每帧用 `UpdateLayeredWindow(pptDst = (x, y + slide))` 上屏，入场/退场时
`slide` 非零；`WM_MOVE` 处理又会把收到的坐标写回 `state.x/y`。如果 ULW 移动窗口时
同步发 `WM_MOVE`，`state.y` 会被改成含位移的值，下一帧再叠加一次位移，每次入场
整体下移几十像素（Wine 的 `UpdateLayeredWindowIndirect` 内部走 `set_window_pos`，
看起来很像会这样）。

## 实测结论（Windows 10 19045，2026-10-28）

用真实窗口、关闭离屏接缝，Show 后逐帧派发 `WM_TIMER`，每帧对比 `state.y` 与
`GetWindowRect`：

- 入场期间 `GetWindowRect().top` 依次为 1280、1278、1277…1270，说明 ULW 确实在移动窗口；
- 同期 `state.y` 始终是配置位 1270，三轮 Show/Hide 之后仍是 1270。

即 **ULW 通过 `pptDst` 改变分层窗口位置不会发 `WM_MOVE`**，字幕不存在位移漂移，
不需要为此"修复"。本项目里会触发 `WM_MOVE` 的是 `SetWindowPos`（命令摆位、DPI 调整）
与系统拖拽（HTCAPTION 模态循环）。

## 做法

- 重构原生窗口时保留"上屏位置 = 基准位置 + 过渡位移、基准只由命令/拖拽/DPI 改写"
  的语义即可；不要为这个不存在的漂移改动 `WM_MOVE` 处理。
- 验证这类"系统会不会同步回调"的疑问，直接用真实窗口打日志对比，不要按 Wine 源码推断
  Windows 行为。
