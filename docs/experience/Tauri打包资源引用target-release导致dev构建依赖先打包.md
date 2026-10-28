# Tauri 打包资源引用 target/release 导致 dev 构建依赖先打包

## 现象

- 新克隆的仓库直接 `npm run tauri:dev` 或 `npm run test:rust`，tauri-build 报
  `resource path target/release/onnxruntime.dll doesn't exist`（macOS 是 frameworks 找不到
  `target/release/*.dylib`），必须先成功打一次包。直接 `cargo check` 同样受影响。
- 清空 `target/release` 后同样失败，所以本地无法放心回收 release 产物。

## 根因

- tauri-build 在 `build.rs` 里对**每个 profile** 都复制 `bundle.resources` 与 `bundle.macOS.frameworks`，
  缺文件就报错，不只在打包时才读取。
- 打包配置把 sherpa 运行库的来源写成了 release 的输出目录，于是 debug 构建也依赖
  一次成功的 release 构建。运行时并不需要这份复制：sherpa-onnx-sys 会把运行库放到当前
  profile 的输出目录，并在 macOS 加 `@loader_path` rpath。

## 解决

- `build.rs` 的 `distribute_sherpa_runtime_libs` 把当前 profile 输出目录里的运行库同步到
  固定的 `src-tauri/target/runtime-libs/`（内容未变不重写，不在清单里的旧文件删除），配置只引用这里。
- 顺序依赖：`build.rs` 要等 sherpa-onnx-sys 的构建脚本先复制好运行库。Cargo 只对**直接**
  依赖里声明了 `links` 的包保证构建脚本先跑，所以 `sherpa-onnx-sys` 必须作为直接的普通依赖
  （代码不使用它，Cargo.toml 已注释）。放在 `[build-dependencies]` 只能碰巧保证宿主机构建，
  `--target` 交叉编译时没有顺序保证。
- Windows 的 `resources` 用映射写法 `"target/runtime-libs/*.dll": "target/release"`：来源是同步目录，
  安装位置仍是 `target\release\*.dll`，NSIS 钩子与安装包布局不变。macOS 的 frameworks
  按文件名进入 `Contents/Frameworks`，只改来源路径。
- `run-rust-tests.mjs` 不再清空测试时的 resources 与 frameworks，测试走真实的打包资源路径；
  sidecar（`externalBin`）仍需先构建，测试脚本会清空它，直接 `cargo check` 则要先跑
  `npm run context-probe:build`。
- 增删运行库时同步三处：`build.rs` 的清单常量、`tauri.macos.conf.json` 的 frameworks、
  `nsis-hooks.nsh` 的复制与删除。

## 验证

- 挪走 `target/release` 后 `npm run test:rust` 仍能编译并通过。
- 打包后用 `7z l <setup.exe>` 核对安装包：DLL 仍在 `target\release\`，许可证仍在
  `resources\licenses\`，与改动前的文件清单一致。
