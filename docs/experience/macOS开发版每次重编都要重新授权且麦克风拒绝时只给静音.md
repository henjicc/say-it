# macOS 开发版每次重编都要重新授权，且麦克风被拒绝时只给静音

## 现象

- `npm run tauri:dev` 下测麦克风，电平一直为零，界面没有任何报错。
- 之前已经授权过辅助功能、输入监控，改完 Rust 代码后又要重新授权。

## 根因

- 开发版 `target/debug/say-it` 是 ad-hoc 签名。macOS 认 ad-hoc 签名的程序靠的是代码哈希，不是签名标识，所以即使 `codesign -dv` 显示的 `say_it-<哈希>` 标识不变，每次重编后代码哈希一变，系统也会把它当成新程序，之前的授权全部不算数。
- 麦克风没有授权时，CoreAudio 打开输入设备不会报错，只送全零数据，应用如果不主动检查，就只能看到静音。
- 隐私授权记在「负责进程」名下，而不是实际调用麦克风的进程。从 Claude 桌面应用等工具启动时，启动链是 `disclaimer → npm → node → 开发版`，负责进程是 npm/node：它们没有麦克风用途说明，系统直接静默拒绝，不弹窗，「系统设置 → 麦克风」列表里也不会出现任何条目。从「终端」启动时，负责进程是终端，授权记在终端名下。

## 做法

- 打开麦克风前（`backend_mic::prepare_backend_mic_input`）先用 `AVCaptureDevice` 查授权：已拒绝时返回可读错误，提示去「系统设置 → 隐私与安全性 → 麦克风」打开；没决定过时弹出系统授权并等待用户作答。检查放在 `start_backend_mic_inner` 取启停锁之前，等待期间不挡其他会话停止麦克风；它也不能在主线程上调用。
- `npm run tauri:dev` 通过 cargo runner（`scripts/macos-dev-runner.sh`，由 `run-tauri.mjs` 只在 dev 时设置）在启动前给开发版签名：用一张自签名开发证书，身份固定为「`com.henjicc.sayit.dev` + 证书指纹」，授权一次后跨编译保留。证书不需要系统信任，放在 `工作区/dev-signing/` 的独立钥匙串里，不进钥匙串搜索列表，删掉该目录会重新生成，生成后需要再授权一次。
- runner 最后 exec 成启动器 `scripts/macos-dev-launcher.c`（首次运行时编译到 `工作区/dev-signing/`）。启动器用 `responsibility_spawnattrs_setdisclaim` 拉起开发版，让开发版成为自己的负责进程，授权就记在固定签名的开发版名下。
- tauri dev 重编时会结束启动器：普通结束信号由启动器转给开发版；启动器被强制结束时，开发版靠 `main.rs` 的 `exit_with_dev_launcher`（仅在 `SAYIT_DEV_LAUNCHER` 已设置时启用）发现父进程变化后自行退出，否则旧实例会占着单实例锁。
- 用 `security create-keychain` 时务必给绝对路径，相对路径会建到 `~/Library/Keychains/` 下。
- 授权被拒绝后，系统不会再弹申请框，`requestAccess` 直接返回未允许；只能由用户到系统设置里打开。
- 安装版的签名身份固定为 `com.henjicc.sayit`，授权一次即可，不受上面的影响。
