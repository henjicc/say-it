# macOS 原生快捷键、系统音频与 Vision OCR 接入边界

## 平台能力边界

- macOS 透明 WebView 窗口需要 Tauri 的 `app.macOSPrivateApi`；仅给窗口 CSS 设置 `background: transparent` 不足以清除 WKWebView/NSWindow 的白色底层。该能力会影响 Mac App Store 审核，当前项目使用 GitHub Release 分发时可接受，但若未来上架商店必须重新评估。
- 悬浮窗不能按完整显示器高度推算底边。macOS 应使用 `NSScreen.visibleFrame`，它会按当前 Dock、菜单栏与刘海安全区域动态排除不可用空间；不要缓存结果，Dock 尺寸、位置和自动隐藏设置都可能变化。
- Caps Lock 不能交给 Tauri 全局快捷键注册。要避免切换大小写状态，必须使用可丢弃事件的 Quartz `CGEventTap`，并请求“辅助功能”权限；设置页录制 Caps Lock 时也要由同一事件过滤器吞键并向前端上报。
- `kCGSessionEventTap` 收到 Caps Lock 时系统锁定状态已经改变，单纯从回调返回 `NULL` 只能阻止后续投递，不能恢复大小写状态或键盘灯。监听启动时应记录 `IOHIDGetModifierLockState`，每次触发后用 `IOHIDSetModifierLockState` 写回；初始化时同时探测写权限，不能在无法恢复状态时假装快捷键注册成功。
- macOS 上不能在 Tokio/阻塞工作线程里用 `enigo::Key::Unicode` 发送粘贴快捷键。enigo 会通过 Text Services Manager 查询当前键盘布局，而该 API 强制要求主队列，违规调用会触发 `dispatch_assert_queue` 并以 `SIGTRAP` 直接终止进程。听写完成后的 Command+V 应使用不查询输入法布局的 CoreGraphics 物理键事件，或显式调度到主线程；不能依赖 Rust 错误处理捕获这种系统级断言。
- ScreenCaptureKit 的系统音频输出、`capturesAudio`、采样率和声道配置从 macOS 13 起可用。应用仍可保持 macOS 11 的整体最低版本，但系统音频入口必须明确标注 13+，运行时也必须用可用性检查返回可操作错误。
- macOS 系统 OCR 使用 Vision `VNRecognizeTextRequest`。Vision 的文本框以左下角为原点，进入公共 `OcrTextBlock` 前必须转换为左上角原点并收敛到 0～1。
- 当前窗口截图在 macOS 14+ 使用 ScreenCaptureKit `SCScreenshotManager`；旧系统只能动态查找已废弃的 `CGWindowListCreateImage`，避免在 macOS 15 SDK 下直接引用已标记 unavailable 的符号。

## 权限与隐私

- `Info.plist` 必须声明 `NSScreenCaptureUsageDescription`；系统音频和窗口 OCR 共用“屏幕与系统音频录制”权限。
- 窗口 OCR 前必须通过辅助功能 API 检查焦点控件的 `kAXSecureTextFieldSubrole`。无法确认输入区域安全性时保守失败，禁止继续截图或调用第三方 OCR。
- 首次授权后系统可能要求重启应用；不要把“已弹出权限提示”当成能力已经可用。

## 构建与验证

- 原生 Objective-C 桥接由 `build.rs` 编译，Tauri 构建脚本需设置 `MACOSX_DEPLOYMENT_TARGET=11.0`，否则命令行工具可能把二进制最低版本提升到当前 SDK 版本。
- 构建后用 `otool -l` 检查 `LC_BUILD_VERSION minos`，并用 `plutil` 确认权限说明实际进入 `.app/Contents/Info.plist`。
- 自动化测试只能覆盖桥接编译、Vision 图片输入和领域状态机；Caps Lock 吞键、权限弹窗、真实系统音频与目标窗口 OCR 必须由用户实际操作验证。
- 前端模型目录是异步加载的，但 Zustand Store 会在模块导入时同步初始化。默认模型不能先以空串占位，否则全新数据目录会把空 `asrModel` 导入后端，快捷键虽已触发却会在悬浮窗创建前报“听写模型未登记”。同步初值应直接来自共享模型注册表，后端加载和听写启动仍需修复历史空值。
