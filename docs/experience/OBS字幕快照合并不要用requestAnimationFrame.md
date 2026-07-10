# OBS 字幕快照合并不要用 requestAnimationFrame

## 问题

`syncObsOverlay` 曾用 `requestAnimationFrame` 合并同一轮渲染里原文/译文各自触发的快照推送。但 WebView2（Chromium）在窗口最小化或被完全遮挡时会暂停 rAF 回调，而直播场景下用户很可能把主窗口最小化——此时 ASR 事件仍在到达、`renderSubtitle` 仍在执行，但 rAF 回调永远不触发，OBS 里的字幕会整个冻结不再更新。

## 正确做法

用 `queueMicrotask` + 布尔标志合并：同一个宏任务内的多次调用只推送一次，且微任务不受窗口可见性/最小化影响。

```ts
let queued = false;
function syncObsOverlay() {
  if (queued) return;
  queued = true;
  queueMicrotask(() => { queued = false; /* publish */ });
}
```

## 触发条件

任何"后台窗口仍需持续向外推送数据"的路径（OBS 字幕、悬浮窗同步等），都不要依赖 rAF 或长间隔 setTimeout（Chromium 对隐藏页面的定时器也会节流到约 1 次/秒），事件驱动 + 微任务合并才可靠。
