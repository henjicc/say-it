# macOS 原生快捷键、系统音频与 Vision OCR 接入边界

## 平台能力边界

- macOS 透明 WebView 窗口需要 Tauri 的 `app.macOSPrivateApi`；仅给窗口 CSS 设置 `background: transparent` 不足以清除 WKWebView/NSWindow 的白色底层。该能力会影响 Mac App Store 审核，当前项目使用 GitHub Release 分发时可接受，但若未来上架商店必须重新评估。
- 悬浮窗不能按完整显示器高度推算底边。macOS 应同时读取 `NSScreen.frame` 与 `visibleFrame` 且不要缓存：底部 Dock 会抬高 `visibleFrame.minY`，底部锚点应据此动态避让；Dock 位于左/右侧时则按完整 `frame` 的水平中心和底边定位，避免无关的横向偏移。听写窗口本身还有 24px 透明底部内边距，设置视觉间距时要从窗口偏移中扣除，不能在 Dock 安全区之外再次叠加一整份桌面边距。
- Caps Lock 不能交给 Tauri 全局快捷键注册。要避免切换大小写状态，必须使用可丢弃事件的 Quartz `CGEventTap`，并请求“辅助功能”权限；设置页录制 Caps Lock 时也要由同一事件过滤器吞键并向前端上报。
- `kCGSessionEventTap` 收到 Caps Lock 时系统锁定状态已经改变，单纯从回调返回 `NULL` 只能阻止后续投递，不能恢复大小写状态或键盘灯。监听启动时应记录 `IOHIDGetModifierLockState`，每次触发后用 `IOHIDSetModifierLockState` 写回；初始化时同时探测写权限，不能在无法恢复状态时假装快捷键注册成功。
- 听写时焦点位于其他应用，WebView 的键盘事件和普通窗口快捷键都收不到 Esc。macOS 必须在听写活动期间把 `kCGEventKeyDown`/`kCGEventKeyUp` 加入 Quartz 事件过滤器，按物理键码 53 触发领域取消并吞掉按下与释放事件；空闲后立即撤销 Esc 监听，避免常驻事件过滤器观察无关键盘输入。取消入口本身不能再限制为 Windows 编译。
- macOS 上不能在 Tokio/阻塞工作线程里用 `enigo::Key::Unicode` 发送粘贴快捷键。enigo 会通过 Text Services Manager 查询当前键盘布局，而该 API 强制要求主队列，违规调用会触发 `dispatch_assert_queue` 并以 `SIGTRAP` 直接终止进程。听写完成后的 Command+V 应使用不查询输入法布局的 CoreGraphics 物理键事件，或显式调度到主线程；不能依赖 Rust 错误处理捕获这种系统级断言。
- “逐字输入”同样不能继续走 enigo 的 macOS Unicode 路径。应使用 `CGEventKeyboardSetUnicodeString` 直接发送 UTF-16，并按组合字符边界分批，避免拆开代理对、emoji 或带组合符号的文字；事件必须清空修饰键标志，防止用户仍按着 Command/Option 时改变注入语义。
- macOS 粘贴注入不能只用 `get_text()` 备份剪贴板：图片、文件和富文本会被永久覆盖。应在主线程按 `NSPasteboardItem` 的全部类型和原始数据建立快照，完成 Command+V 后再恢复；恢复前必须核对 `changeCount`，若用户或其他应用已复制新内容则放弃恢复，不能用旧快照覆盖新剪贴板。
- ScreenCaptureKit 的系统音频输出、`capturesAudio`、采样率和声道配置从 macOS 13 起可用，运行时仍应使用可用性检查返回可操作错误。当前 sherpa-onnx 1.13.5 所带 ONNX Runtime 的 `LC_BUILD_VERSION minos` 为 15.5，应用的最低系统版本必须与最严格的内嵌动态库一致；不能只检查主程序的 minos 后继续宣称支持 macOS 11。
- ScreenCaptureKit 的采样回调持有 Rust context 裸指针，停止采集时不能先释放指针再等待异步回调自然消失。应先在 `sampleHandlerQueue` 上同步设置回调与 context 为空，以串行队列屏障等待正在执行的回调结束，再调用 `stopCapture`；这样即使停止超时，后续样本也不会访问已经释放的 Rust 状态。
- ScreenCaptureKit 运行中可能因显示器断开、权限变化或系统服务异常调用 `SCStreamDelegate.stream(_:didStopWithError:)`。错误回调应转发到与采样相同的串行队列，再通知 Rust worker 关闭所有音频发送端并保留原始错误；实时字幕的原始音频通道若在会话仍活动时关闭，必须进入失败清理并把错误投影到界面，不能继续停留在“运行中”却只输出静音。
- CPAL 麦克风流也会在蓝牙设备断连、默认输入切换或 CoreAudio 服务异常时异步报告错误。错误回调不能只写日志：应只转发首个错误给采集 worker，关闭原始音频通道并保留错误；听写、实时字幕、模型对比和音频调校各自负责把仍活动的会话切到失败状态、停止 ASR 并释放音频租约。启动命令还应等待 `build_input_stream` 与 `play` 的确认，避免底层启动已经失败却向前端返回成功。
- macOS 系统 OCR 使用 Vision `VNRecognizeTextRequest`。Vision 的文本框以左下角为原点，进入公共 `OcrTextBlock` 前必须转换为左上角原点并收敛到 0～1。
- macOS 的低内存文本提取可通过 Accessibility API 读取焦点控件的 `AXSelectedText`、`AXValue` 和 `AXSelectedTextRange`。跨进程 AX 调用必须设置短消息超时，密码控件要在读取正文前按 `AXSecureTextField` 保守拦截；拿不到正文时只允许回退到应用名与窗口标题，不能改用剪贴板或静默截图。
- PP-OCR 不能只接在 Windows 场景感知管线中；模型校验、MNN 引擎创建和结果归一化应放到跨平台 OCR 模块，macOS 的通用 OCR 供应商入口才能真正调用本地模型。`ocr-rs` 在 macOS 使用不启用 `mnn-static` 的预编译 universal MNN，下载归档必须固定 SHA-256，避免构建期供应链内容漂移。
- 上下文调试不是单纯开放窗口入口：调试模块、配置解析、`begin_debug_capture` 和完整等待解析都必须为 macOS 编译；目标应用拥有焦点时再通过临时注册的全局 `Control+Shift+F8` 触发捕获。调试窗口关闭后立即注销快捷键，避免开发调试入口常驻占用系统快捷键。
- 当前窗口截图在 macOS 14+ 使用 ScreenCaptureKit `SCScreenshotManager`；旧系统只能动态查找已废弃的 `CGWindowListCreateImage`，避免在 macOS 15 SDK 下直接引用已标记 unavailable 的符号。

## 权限与隐私

- `Info.plist` 必须声明 `NSScreenCaptureUsageDescription`；系统音频和窗口 OCR 共用“屏幕与系统音频录制”权限。
- 窗口 OCR 前必须通过辅助功能 API 检查焦点控件的 `kAXSecureTextFieldSubrole`。无法确认输入区域安全性时保守失败，禁止继续截图或调用第三方 OCR。
- 首次授权后系统可能要求重启应用；不要把“已弹出权限提示”当成能力已经可用。
- 打开 macOS 上下文调试窗口时，应先快速预检辅助功能与屏幕录制权限；缺失时调用 `AXIsProcessTrustedWithOptions` / `CGRequestScreenCaptureAccess` 尝试触发系统授权入口，并在调用后再次预检。授权仍未生效时不要创建调试窗口或注册调试快捷键，直接提示用户到“系统设置 → 隐私与安全性”授权。
- 普通听写中的上下文 OCR 只做快速预检，不在每次听写重复弹权限提示；缺失权限时返回可读诊断并跳过 OCR，不应让语音输入进程因底层截图错误崩溃。开发态 `npm run tauri:dev` 与打包后的 `.app` 可能对应不同的 TCC 权限主体，二者需分别授权并分别验证。
- macOS 从 Finder 双击关联文件不会可靠地把文件路径放进 `argv`；Tauri 会通过 `RunEvent::Opened { urls }` 交付 Apple 文件打开事件。`.sayit` 导入必须同时处理该事件、启动参数和单实例回调，并统一进入同一待安装队列。
- 应用规则从本地选择 `.app` 时不能直接拿包目录名作为进程匹配键；`.app` 名与 `CFBundleExecutable` 经常不同。应由原生层通过 `NSBundle.executableURL` 解析实际进程名，同时读取 `CFBundleDisplayName`/`CFBundleName` 作为显示名。
- macOS 字幕字体列表不能沿用 Windows 注册表实现；应在主线程通过 `NSFontManager.availableFontFamilies` 读取。自定义数据目录迁移的剩余空间检查也不能静默跳过，应读取卷的 `NSURLVolumeAvailableCapacityForImportantUsageKey`，失败时再降级到普通可用容量键。
- 新安装的 macOS 字幕默认字体应使用系统自带的 `PingFang SC`，不能继续写入 Windows 的 `Microsoft YaHei`；字体下拉在加载系统列表后仍要保留当前已保存但本机缺失的字体项，避免设置值存在而控件显示为空。
- macOS 普通应用在销毁所有窗口后仍会保留 Dock 图标。“静默启动后只驻留托盘/状态栏”不能只复用 Windows 的隐藏窗口逻辑：应在确认由开机自启且启用静默启动时，于事件循环启动前隐藏 Dock；用户从状态栏、文件关联或单实例入口打开主窗口时再恢复 Dock 身份。系统自启状态读取失败也必须向界面返回错误，不能静默当作“未开启”。

## 构建与验证

- 原生 Objective-C 桥接由 `build.rs` 编译，Tauri 构建脚本需显式设置与应用声明一致的 `MACOSX_DEPLOYMENT_TARGET`，否则命令行工具可能把二进制最低版本提升到当前 SDK 版本。
- 构建后要用 `otool -l` 检查主程序及 `Contents/Frameworks` 内每个 Mach-O 的 `LC_BUILD_VERSION minos`，取其中最大值与 `LSMinimumSystemVersion` 对齐；仅检查主程序会漏掉应用启动时由 dyld 强制加载的本地模型运行库。还需用 `plutil` 确认权限说明实际进入 `.app/Contents/Info.plist`。
- 自动化测试只能覆盖桥接编译、Vision 图片输入和领域状态机；Caps Lock 吞键、权限弹窗、真实系统音频与目标窗口 OCR 必须由用户实际操作验证。
- 本地 PP-OCR 应使用随包测试图和真实模型做一次推理测试，不能只以 Rust 编译或模型文件存在作为可用证据。
- 前端模型目录是异步加载的，但 Zustand Store 会在模块导入时同步初始化。默认模型不能先以空串占位，否则全新数据目录会把空 `asrModel` 导入后端，快捷键虽已触发却会在悬浮窗创建前报“听写模型未登记”。同步初值应直接来自共享模型注册表，后端加载和听写启动仍需修复历史空值。
