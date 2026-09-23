# 静默启动预创建 WebView 导致额外常驻占用

## 根因与处理

Tauri 平台配置中的主窗口虽然 `visible: false`，但 `create` 默认为 true，仍会在
`setup` 前创建浏览器。原静默启动流程读取设置后再销毁主窗口，浏览器子进程退出，
主进程却已加载 `EmbeddedBrowserWebView.dll` 并付出相关初始化与内存开销。

Windows/macOS 主窗口配置改为 `create: false`。普通启动通过已有 `ensure_main_window`
入口按需创建，静默启动则保留无主窗口状态。主窗口 ready 握手、托盘/单实例唤起、
位置恢复与原生材质沿用原流程；显式预创建窗口的外部配置仍兼容原有登记分支。

这解决的是**尚未打开过主界面的静默启动**。打开过主界面后再关闭，WebView 组件仍可能
留在宿主进程；不能把本轮收益宣传成所有关闭窗口场景的收益，也不能把宿主进程的
全部占用当作 Rust 堆。

## 测量与验证边界

2026-09-24，Windows x64，构建任务结束后的三组交替测量，中位数如下：

| 主进程指标 | 优化前 | 按需创建后 |
| --- | ---: | ---: |
| 私有提交峰值 | 24.59 MiB | 8.37 MiB |
| 观察结束时私有提交 | 12.14 MiB | 8.33 MiB |
| 私有驻留 | 7.55 MiB | 4.33 MiB |
| 工作集（含可共享页面） | 36.17 MiB | 19.45 MiB |
| 累计 CPU 时间 | 265.63 ms | 62.50 ms |
| 线程数 | 47 | 38 |
| 加载的 WebView 模块 | `EmbeddedBrowserWebView.dll` | 无 |

原始数据为 `工作区/performance/tray-final-{baseline,optimized}-{1,2,3}.json`。
基线程序基于 `77028aa`，只覆写测试标识；SHA256 为
`9594484C27D1B0841D6F743A33C51ACF4565507BE2F0CBDB99B27CC4490CDA7C`。
按需创建程序 SHA256 为
`7525F834491EFEA4F83EFB1191677BB5DF52B303B912A7995A532B943BD21F44`。

- 使用独立标识 `com.henjicc.sayit.perf20260924` 构建测试程序，测试数据目录只放
  `schema_version: 4` 与 `startup.silent_start: true` 的初始配置，未修改日常应用设置。
- 使用 release 构建，前后版本交替启动三组；每次新进程启动后观察约八秒。操作系统
  文件缓存和 WebView 磁盘缓存不清空，因此这里的“冷启动”只指新进程启动。
- `scripts/采样应用启动.ps1` 记录主进程私有提交、私有驻留、历史峰值、累计 CPU 时间、
  线程数、WebView 模块以及采样曲线。报告不包含已退出子进程的历史用量，也不是启动
  到可交互的延迟测量。
- `scripts/测量进程内存.ps1` 对当前进程树补充私有驻留、可共享驻留、峰值、I/O 和线程
  切换计数。速率使用计数器实际时间间隔；新建、退出或复用的 PID 单独标记。工作集
  直接相加会重复计算共享页面，线程切换次数也不能当作精确唤醒次数。
- Windows 实际验证静默启动、首次通过单实例唤起主界面、两次关闭后重开；每次都只有
  一个主窗口且首页控件就绪。记录在 `工作区/performance/window-reopen-stage3.json`。
- macOS 配置已同步修改，但没有本机运行证据；不能声称已完成 macOS 验证。
- Rust 发布模式回归 567 项通过，0 失败；普通发布版也重新构建并启动验证。

计数器口径参考 [Microsoft 内存性能信息](https://learn.microsoft.com/windows/win32/memory/memory-performance-information)
与 [Windows 工作集](https://learn.microsoft.com/windows/win32/memory/working-set)。
