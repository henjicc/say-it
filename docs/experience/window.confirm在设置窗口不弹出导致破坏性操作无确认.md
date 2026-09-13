# window.confirm 在设置窗口不弹出，导致破坏性操作直接执行

## 现象

在「设置 → 高级 → 重置数据」按钮的 `onClick` 里用 `window.confirm(...)` 做二次确认，代码逻辑本身没问题（`if (!window.confirm(...)) return;`），但实测点击按钮后**没有出现任何确认弹窗**，直接执行了破坏性操作（清空数据 + 重启）。

## 根因（未彻底定位，先记录现象和规避方式）

没有找到项目里对 `window.confirm`/`alert` 的显式覆盖，也没有会阻断它的 CSP 配置；大概率是这套无边框（`decorations: false`）、自绘窗口的 Tauri/WebView2 环境下，原生 `window.confirm()` 弹出的系统对话框存在窗口归属/层级问题（可能被主窗口挡住、或压根没有正常呈现），而不是代码逻辑错误。

`PluginManagerPanel.tsx` 里"卸载插件"的二次确认**没有用 `window.confirm`**，而是用了 `Modal` 组件 + `pendingUninstall` 状态做应用内确认——大概率是之前已经踩过这个坑，只是没有留下记录。

## 规避方式

**这个项目里，任何需要二次确认的破坏性操作，一律用 `components/ui/Modal.tsx` 组件做应用内确认弹窗，不要用 `window.confirm`/`window.prompt`。** 参考 `PluginManagerPanel.tsx` 的 `pendingUninstall` 模式：用一个 state 记住"待确认的操作对象"，点击触发按钮时只设置这个 state（不直接执行），`Modal` 里放"确认"（`variant="dangerHover"`）和"取消"（`variant="primary"`，`autoFocus`）两个按钮。

## 待办 / 风险提示

项目里其他仍在用 `window.confirm` 做确认的地方（例如 `SettingsHistoryPanel.tsx` 的"清空全部历史/清空使用统计/清空学习记忆"、`SettingsAdvancedPanel.tsx` 的"清空诊断日志"等）**没有逐一验证是否也存在同样的问题**——不排除这些按钮点击后也是直接执行、没有真正弹出确认框。如果之后有人反馈"点了清空按钮没确认就执行了"，大概率就是这个坑，直接按上面的方式换成 `Modal` 即可，不用重新排查。
