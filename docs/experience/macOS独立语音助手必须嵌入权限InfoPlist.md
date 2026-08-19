# macOS 独立语音助手必须嵌入权限 Info.plist

## 触发条件

主应用通过子进程调用 `Speech` 等隐私敏感框架时，子进程不会继承主应用 `Info.plist` 里的用途说明。即使主应用已经声明 `NSSpeechRecognitionUsageDescription`，无自身 Bundle 身份的助手仍会被 TCC 直接终止，调用方通常只能观察到 EOF 或笼统的“未能启动”。

## 正确做法

- 为命令行助手准备固定的 `CFBundleIdentifier` 和用途说明。若助手只是主应用的子进程并需要共用同一项 TCC 授权，Bundle ID 与代码签名标识应复用主应用身份，避免产生系统无法定位的权限主体。
- 链接时将 plist 写入 Mach-O 的 `__TEXT,__info_plist`；开发构建也要使用固定代码签名标识。
- 助手在接触隐私 API 前通过 `Bundle.main` 自检 Bundle ID 和用途说明，避免由系统崩溃代替可读错误。
- 开发态不能直接运行仓库里的裸 Mach-O；应把助手放进具有独立开发 Bundle ID 的最小 `.app` Bundle，并向 Launch Services 注册后调用，否则 TCC 可能把未注册的开发产物直接判为拒绝。最终包内则使用复用主应用身份、已随主应用签名的助手副本。
- 旧版 `SFSpeechRecognizer` 的首次授权必须从 Launch Services 启动的开发 Bundle 内请求；直接执行 Bundle 内 Mach-O 只适合授权后的音频管道。开发构建仅在助手二进制变化时重置该开发 Bundle 的授权并触发一次系统弹窗，不能每次启动都重签和重置。
- 构建脚本和应用包校验都执行同一个无权限副作用的 `--self-check`，同时覆盖开发产物与最终包内产物。
- 异常退出且没有协议终态时，调用方必须显示进程状态或 stderr，不能退化成未知失败。

Apple 对命令行目标的标准机制是 `CREATE_INFOPLIST_SECTION_IN_BINARY`；直接使用编译器时可通过链接器 `-sectcreate __TEXT __info_plist` 实现等价结果。
