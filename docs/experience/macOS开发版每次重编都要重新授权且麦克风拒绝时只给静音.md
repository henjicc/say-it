# macOS 开发版每次重编都要重新授权，且麦克风被拒绝时只给静音

## 现象

- `npm run tauri:dev` 下测麦克风，电平一直为零，界面没有任何报错。
- 之前已经授权过辅助功能、输入监控，改完 Rust 代码后又要重新授权。

## 根因

- 开发版 `target/debug/say-it` 是 ad-hoc 签名。macOS 认 ad-hoc 签名的程序靠的是代码哈希，不是签名标识，所以即使 `codesign -dv` 显示的 `say_it-<哈希>` 标识不变，每次重编后代码哈希一变，系统也会把它当成新程序，之前的授权全部不算数。
- 麦克风没有授权时，CoreAudio 打开输入设备不会报错，只送全零数据，应用如果不主动检查，就只能看到静音。

## 做法

- 打开麦克风前（`backend_mic::prepare_backend_mic_input`）先用 `AVCaptureDevice` 查授权：已拒绝时返回可读错误，提示去「系统设置 → 隐私与安全性 → 麦克风」打开；没决定过时发起系统授权，非主线程上等待用户作答，主线程上只发起、不阻塞。
- `npm run tauri:dev` 通过 cargo runner（`scripts/macos-dev-runner.sh`，由 `run-tauri.mjs` 只在 dev 时设置）在启动前给开发版签名：用一张自签名开发证书，身份固定为「`com.henjicc.sayit.dev` + 证书指纹」，授权一次后跨编译保留。证书不需要系统信任，放在 `工作区/dev-signing/` 的独立钥匙串里，不进钥匙串搜索列表，删掉该目录会重新生成，生成后需要再授权一次。
- runner 必须 `exec` 成开发版本身：cargo run 在 Unix 上也是 exec，tauri dev 重编时直接结束这个进程，多一层壳会留下旧实例。
- 用 `security create-keychain` 时务必给绝对路径，相对路径会建到 `~/Library/Keychains/` 下。
- 授权被拒绝后，系统不会再弹申请框，`requestAccess` 直接返回未允许；只能由用户到系统设置里打开。
- 安装版的签名身份固定为 `com.henjicc.sayit`，授权一次即可，不受上面的影响。
