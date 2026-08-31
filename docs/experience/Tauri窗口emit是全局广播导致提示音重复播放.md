# Tauri 窗口 emit 是全局广播，提示音需定向派发

Tauri 2 的 `Emitter::emit` 即使调用于 `WebviewWindow`，仍向所有事件目标广播。悬浮球和常驻但隐藏的指示器都订阅 `dictation-indicator-play-cue` 时，两边都会播放一次，不能靠可见性判断是否会出声。

提示音必须通过 `app.emit_to(窗口标签, 事件, 载荷)` 发给唯一承载窗口，接收端也必须使用 `listen(..., { target: 窗口标签 })`。仅修改发送端不够：JavaScript `listen` 默认是 `Any`，Rust `match_any_or_filter` 会让这些监听器继续收到定向事件。[Tauri 事件接口](https://v2.tauri.app/reference/javascript/api/namespaceevent/)。

普通听写发给指示器、悬浮球听写发给悬浮球；指示器不存在时，明确回退到主窗口。前端通过 `useCuePlayback` 集中限定目标，波形也应在两端限定窗口。不能用“窗口隐藏”代替事件隔离。

提示音的调用次数和声音起音次数要分别验证。按单声反馈要求，默认升调/降调使用一个振荡器连续滑音，只有显式选择“内置·双响”才调度两次起音。回归测试应同时覆盖隐藏指示器、悬浮球、StrictMode 清理及音频调度，不能只检查发送端代码。

悬浮球的 pointerup 只结束指针跟踪，click 是唯一业务激活入口；拖动后的合成 click 要跳过，键盘激活保留。否则指针抬起引发状态更新后，同一动作的 click 可能落入另一个阶段的操作分支。
