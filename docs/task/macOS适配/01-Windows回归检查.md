# Windows 回归检查

macOS 适配期间（`4c0c544` 到 `8b365fa`）改动了一些两个平台共用的代码。这些提交是在 Mac 上写的，Windows 只跑过 CI，没有真机运行过。下次在 Windows 上开工时，先按本清单逐项检查。每项完成后勾掉并提交；全部完成后，在文件开头注明「已完成」。

## 1. 自动检查（先跑，作为基线）

```powershell
git pull --rebase
npm install
npm run check
```

- [x] `npm run check` 全部通过。已知原有失败：`npm run test:scripts` 因 `CHANGELOG.md` 缺 `0.5.0-beta.4` 的更新日志而失败，与本次改动无关，见第 4 节。
- [x] 原生悬浮窗像素快照逐字节比对（悬浮球的镜像逻辑移进了 core，Windows 的包装层也改了）：

  ```powershell
  $env:SAYIT_OVERLAY_SNAPSHOTS='verify'; npm run test:rust -- --bin say-it native_snapshot; Remove-Item Env:SAYIT_OVERLAY_SNAPSHOTS
  ```

- [x] 本地整理推理进程（构建脚本改成了按平台分支）：`npm run llm-engine:build`，然后 `cargo test --release --manifest-path src-tauri/Cargo.toml -p sayit-llm-engine`。
- [ ] 安装包（`src-tauri/Cargo.toml` 新增了 `[profile.release.build-override] strip = false`）：`npm run tauri:build` 能成功，安装包大小与上一版相近，安装后能正常启动。

## 2. 需要真机操作确认的改动

| 改动 | 提交 | 怎么确认 |
|---|---|---|
| 悬浮球请求镜像 `OrbMirrors` 移入 `overlay-core`，`native_orb.rs` 两个平台共用；Windows 的 `cursor_over` 改为 `cursor_px` 加共用判断 | `b36f0e0` | 拖动悬浮球，退出后重开，位置不变；把悬浮球拖到不同缩放比例的显示器上，大小正确；鼠标手势只在指针位于球上时才被球拦截 |
| 字幕条关闭按钮用的 `APP` 从 `native_win/subtitle.rs` 移到 `native_subtitle.rs` | `b36f0e0` | 开启实时字幕，点字幕条上的关闭按钮，字幕停止 |
| 官方模型安装不再传预览标记（之前所有平台从官方列表安装都会报「说吧包在预览后已发生变化」） | `f6b1354` | 「模型 → 本地模型与插件」里从官方列表安装任意一个模型包，能装上 |
| 模型下载带上 User-Agent（之前 ModelScope 对大文件返回 403） | `2bd8241` | 从官方列表下载 Qwen3-ASR 或整理模型，进度正常走完 |
| 插件页安装结束后不再残留「正在安装：x / x」 | `52f0b05` | 装完一个模型包，页面底部的进度提示消失 |
| 「环境状态」从设置页内展开改为弹窗，每次打开、切回软件时重新检查 | `0233705` | 「设置 → 通用 → 环境状态」弹出对话框；Windows 上不应出现屏幕录制、输入监控两项 |
| 页面骨架 `MasterDetail` 与历史页多包了一层容器（为 Mac 的拖动条），Windows 上拖动条不渲染 | `4595a41` | 专业模式各页的中栏与检查器间距、滚动与改前一致；历史页能正常滚动；标题栏仍显示图标和「说吧！」 |
| 边说边整理带上已读到的上下文（只对立即读取方式生效；Windows 截图 OCR 是延迟识别，不受影响） | `8b365fa` | 上下文读取方式选「读取焦点文字」、智能处理选本地整理模型，听写一句较长的话后看开发者追踪 `GET /runs/latest`：最后一条 `localLlm.run` 的 `promptEvaluated` 应只有几十个 token，不是几百个 |
| 硬件信息多了 `unifiedMemory` 字段（Windows 恒为 false） | `b096f0a` | 引导和模型页的硬件描述仍是「显卡名（X GB 显存）· X GB 内存」，推荐结果与改前一致 |
| 晃鼠标唤出的待确认悬浮球改为指针停在球上或拖动中不收起，离开 2 秒后才消失 | `6ce839a` | 鼠标手势选「确认」模式，晃出悬浮球后指针停在球上超过 3 秒不消失；按住拖动一段再松手停在球上仍不消失；移开后约 2 秒收起 |
| OCR 收尾整理移到共用的 `active_app_context/ocr_layout.rs`，保留的多栏改为按栏依次读（本机只能编译 macOS，`ocr.rs` 未在 Windows 编译过） | `4fc8337` | `npm run test:rust` 通过；上下文读取方式选截图识别，在左右分栏的窗口里听写，开发者追踪里的软件上下文先读完一栏再读另一栏，没有图标碎片 |
| macOS 粘贴改为等目标真正读取剪贴板后再恢复；Windows 的 `windows_native::run_paste_worker` 仍是粘贴后固定等 180ms 再恢复，同样会在目标繁忙时丢字，需要在 Windows 上改成延迟渲染（`SetClipboardData` 传空句柄、处理 `WM_RENDERFORMAT`）并真机验证 | `3c181e0` | 应用刚启动后立即听写一句（模型首次加载、机器繁忙），文字能插入；连续听写多次都能插入，原剪贴板内容在插入后恢复 |
| Windows 粘贴改为延迟渲染：剪贴板先只登记文字格式，目标软件来读（`WM_RENDERFORMAT`）后再等 100ms 还原，没人读最多等 1.5 秒；按键前被剪贴板监视程序读走的不算；加了不进剪贴板历史与云剪贴板的标记。本机只在 macOS 上按 Windows 目标做过类型检查，从未在 Windows 运行过 | 本次 | 同上一行的场景；另外打开 Win+V 剪贴板历史，听写后历史里不出现听写的文字；在记事本、浏览器、微信、Office 各听写一次都能插入；开着 Ditto 等剪贴板工具时也能插入 |
| 托盘图标改为只有「声推成字」主体的单色图标，按任务栏深浅（`SystemUsesLightTheme`）选白或黑，指针移到图标上时跟随切换。本机只在 macOS 上编译过，Windows 分支没编译过 | 本次 | 深色任务栏下图标为白色、浅色下为黑色，大小与旁边图标相近、不模糊；在「设置 → 个性化 → 颜色」切换 Windows 模式后，把指针移到托盘图标上，图标换成对应颜色 |

## 3. 审查时提出、留给 Windows 处理的小项

- [x] `src-tauri/src/desktop/native_win/orb.rs` 的 `WindowModel` 现在只剩测试用的悬停记录，可以和字幕一样改成直接用 `Orb`（`after_ops` 里在测试下记录 `hovering()`），并删掉测试里的 `model_with`。必须在 Windows 上改完并跑通快照比对。（`2ae4c74` 改为通用宿主时 `WindowModel` 已删除，测试直接用 `Orb`。）
- [x] Windows 上麦克风被系统关闭（设置 → 隐私 → 麦克风）时，同样没有直达入口。可以参照 Mac 给 `open_privacy_settings` 加 `ms-settings:privacy-microphone`。（已加：整机或「允许桌面应用访问麦克风」关闭时，环境状态与引导页给出「打开系统设置」。）

## 4. 与平台无关的原有问题

- [x] `CHANGELOG.md` 缺 `0.5.0-beta.4` 的更新日志，导致 `npm run test:scripts` 失败。
- [x] `src-tauri/Cargo.toml` 的 `version` 是 `0.4.0`，与 `package.json`、`tauri.conf.json` 的 `0.5.0-beta.4` 不一致（参见 `docs/experience/发布版本时同步更新 Cargo 与前端版本号.md`）。
- [x] `components/ui/Modal.tsx` 打开后不把键盘焦点移进对话框，也不把焦点限制在对话框内，不满足无障碍基线。
