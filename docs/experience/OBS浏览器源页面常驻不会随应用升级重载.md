# OBS 浏览器源页面常驻,不会随应用升级重载

## 问题

OBS 的 Browser Source(`shutdown: false`)一旦加载页面就常驻运行。我们的字幕页面 HTML 内嵌在 Rust 二进制里(`OVERLAY_PAGE`),应用升级重启后,OBS 里仍是**旧版页面**在跑——它自己的 WebSocket 重连逻辑会连上新后端,继续用旧渲染逻辑显示,导致"代码明明改了,OBS 里行为还是旧的"(例如单句替换模式仍然累积换行)。

HTTP 响应加 `Cache-Control: no-store` 没用——问题不是 HTTP 缓存,是页面根本不重新加载。通过 obs-websocket `SetInputSettings` 传相同 URL 也不会触发刷新。

## 正确做法

把页面内容指纹(FNV-1a 哈希)拼进字幕源 URL 的 query 参数(`&v=<hash>`)。页面代码一变 URL 就变,下次 `SetInputSettings` 同步字幕源设置时 OBS 检测到 URL 变化会自动重载页面;同版本内指纹稳定,URL 不变,不会反复刷新闪断。

配合"字幕输出监测激活时强制做一次布局同步"的机制,应用升级后只要开一次预览/字幕,OBS 页面就会自动换新。

## 触发条件

任何"修改了内嵌 OBS overlay 页面(HTML/CSS/JS)但 OBS 里看不到变化"的情况,先怀疑页面没重载,而不是逻辑没生效。手动验证方法:OBS 里对字幕源点"刷新缓存"(属性页 Refresh 按钮)看行为是否变对。
