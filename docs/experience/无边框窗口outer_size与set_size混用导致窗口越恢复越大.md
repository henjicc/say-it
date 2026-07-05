# 无边框窗口 outer_size 与 set_size 混用导致窗口越恢复越大

## 现象

- 从托盘图标打开主窗口时,窗口尺寸会"闪一下";
- 每点击一次托盘图标,窗口就放大一圈,反复点击持续膨胀。

## 根因

Windows 上 `decorations: false` 且 `resizable: true` 的 Tauri 窗口,系统仍会保留一圈**不可见的调整边框**(WS_THICKFRAME,用于拖拽调整大小和阴影):

- `window.outer_size()` 返回的是 GetWindowRect 的完整窗口矩形,**包含**这圈不可见边框;
- `window.set_size()` 设置的是**内容区(inner/client)尺寸**。

主窗口关闭时走"屏外驻留"策略(移到 -32000,-32000),托盘打开时恢复记录的尺寸。若记录用 `outer_size()`、恢复用 `set_size()`,每次"驻留→恢复"窗口就膨胀一圈边框的宽度(约十几像素)。打开瞬间的"闪一下"是 WebView2 异步追赶新窗口尺寸导致的内容重排。

## 正确做法

记录与恢复必须使用同一坐标系:记录用 `inner_size()`,恢复用 `set_size()`;位置则是 `outer_position()` 配 `set_position()`(两者都是外框坐标,本来就对称)。

## 关联问题:hover 残留

屏外驻留(而非 hide/销毁)意味着窗口移走时 WebView 收不到 mouseleave,标题栏关闭按钮的 `:hover` 红色会残留到下次鼠标移动。解决:点击关闭/最小化时在前端主动压制 hover 样式,等鼠标真正在标题栏移动时再恢复(见 `ui/src/components/shell/Titlebar.tsx`)。

另:屏外驻留的窗口没有真正关闭,系统不会播放关闭动画;`hide()` 同样没有动画(系统关闭动画只在窗口销毁时播放),Telegram 等关闭到托盘的应用同样是瞬间消失,属正常现象。
