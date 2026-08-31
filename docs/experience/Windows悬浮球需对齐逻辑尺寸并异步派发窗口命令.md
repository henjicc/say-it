# Windows 悬浮球需对齐逻辑尺寸并异步派发窗口命令

## 胶囊形状：默认最小窗口宽度

无边框且不可调整大小，不等于取消 Windows 的默认最小跟踪宽度。实机读取到的悬浮球客户区与外框均为 **202×98 物理像素**，按钮填满视口后自然成为胶囊。Tao 的 `WM_GETMINMAXINFO` 处理只有在显式指定最小尺寸时才覆盖系统默认值。

Windows 创建悬浮球时必须显式指定 `min_inner_size`，同时关闭不适用于悬浮球的最大化/最小化能力。不能只改 CSS 圆角，也不能根据 `inner_size(width, height)` 的请求值就断言实际窗口是正方形。[Windows 最小跟踪尺寸定义](https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-minmaxinfo)。

## 右侧、底部裁切：分数缩放下的两次取整

150% 缩放下，65 DIP 对应 97.5 物理像素；原生窗口取到 98px，而实机 WebView2 绘制子窗口高为 99px。只把逻辑尺寸取整，仍然会裁切。

Windows 在 28～72 DIP 范围内，选择最接近目标且**逻辑尺寸、缩放后的物理尺寸都为整数**的边长。150% 时选择偶数 DIP，125%/175% 时选择 4 的倍数。创建、缩放、定位、材质半径继续共用尺寸入口；macOS 保留既有计算。不要额外扩张透明窗口，否则系统毛玻璃可能在球外形成光环。极少见的自定义 DPI 若在该范围内没有双重整数解，保留整数 DIP 回退，仍需实机验收。

读取原生几何的诊断线程应先设为 Per-Monitor DPI Aware，否则 `GetWindowRect` 的虚拟化尺寸会混淆物理像素和逻辑像素。应同时比较主窗口、WRY 容器、Chrome 绘制子窗口的客户区与外框，不能只看截图。

## 窗口命令与复用状态

- Windows 上，悬浮球开关、外观调整和首次打开菜单不在 WebView2 同步 IPC 回调中执行，使用 `#[cfg_attr(windows, tauri::command(async))]` 派发；其他平台保留原同步方式。
- [Tauri 窗口构建文档](https://docs.rs/tauri/2.11.0/tauri/webview/struct.WebviewWindowBuilder.html) 明确提示 Windows 同步命令创建 WebView 的死锁风险。开关关闭还包含保存配置与原生窗口操作，不应阻塞 IPC 回调。
- 隐藏复用窗口时若设置了 `ignore_cursor_events(true)`，重新启用必须恢复为 `false`，否则球虽然出现，却无法点击或拖动。

## 验证边界

Rust 回归覆盖 100%～300%（每 25% 一档）和整个尺寸范围的物理像素换算，并覆盖本次 150% 的尺寸案例；前端沿用交互测试。圆形边缘、连续开关、重开后的点击/拖动及跨屏效果仍需用户实机验收，不以编译成功代替这些验收。
