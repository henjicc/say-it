# 闲置线程数量不能直接代表后台 CPU 负载

## 触发条件

看到 Tauri 主进程线程较多时，不能仅按线程数量推断忙轮询、CPU 浪费或可回收内存，也不能将主进程全部私有提交视为 Rust 堆。应先区分线程活动与整组进程的内存分布，再决定是否调整 Tokio 工作线程数。

## 本次证据

2026-09-24，Windows x64、24 个逻辑处理器，应用代码 `1dfbd95`。普通启动后未操作 UI，宿主及 6 个 WebView 子进程仍存在；**不是所有 WebView 已销毁的状态**，也没有验证窗口是否获得前台焦点。采样脚本的 `condition` 是调用方标签，不是自动识别的窗口状态。

发布程序 SHA256：`E0A076F02E13D89D046BB95B36C482CBC319A9AFFFAA5676509EA476E38A18C4`。

两次线程/GPU 采样的整组私有提交约 212 MiB，复采明细如下：

| 指标 | 复采结果 |
| --- | ---: |
| 整组私有提交 | 212.035 MiB |
| 整组私有驻留 | 114.172 MiB |
| 宿主主进程私有提交 | 25.531 MiB |
| 宿主线程数 | 51 |
| 命名为 `sayit-tokio-*` 的线程 | 24 |
| 这 24 个线程的 CPU 周期与上下文切换增量 | 均为 0 |
| 整组 CPU（24 核归一化、该段计数器窗口） | 0.0054% |
| GPU 端点专用内存 | 38.539 MiB |
| GPU 端点共享内存 | 1.938 MiB |

本次观察支持优先区分 WebView 与宿主开销，不支持“线程多所以持续占 CPU”的判断。闲置观察不能证明繁忙负载下适合减少工作线程，线程创建和驻留本身也仍有成本。

原始文件在忽略目录 `工作区/performance/whole-app-idle-current.json`、`whole-app-idle-current-final.json`；基础兼容路径验证结果为 `whole-app-idle-compat.json` 和 `.csv`，JSON 同时记录程序路径与 SHA256。

## 工具与口径

复用 `scripts/测量进程内存.ps1`：`-IncludeThreadActivity` 输出每线程名称、CPU 时间、CPU 周期和上下文切换；`-IncludeGpuActivity` 可选输出两个端点的 GPU 引擎利用率与内存。脚本只申请查询权限，不暂停线程、不调整优先级、不修剪工作集、不操作窗口。基本 JSON 和原有 CSV 输出路径也已实际运行验证。

- WMI 查询耗时会拉长采样间隔，不能把传入的 `CpuSampleSeconds` 当作实际间隔。复采的进程计数器间隔约 24.27 秒，原生线程周期间隔约 29.95 秒，两者单独记录。
- 周期是原生 API 提供的计数，不能换算成耗时；线程切换次数也不是精确唤醒次数。名称可能为空或改变，查询失败返回 null。
- GPU 数据仅描述采样端点，不能代替连续峰值跟踪；不同 GPU 引擎利用率不直接相加。GPU 共享内存也不能不加区分地再加到系统驻留总量。
- 工作集相加重复计算可共享页面，不等于去重后的物理内存。私有提交、私有驻留与共享页面分别保留。
- 进程树按每个父进程的创建时间校验子进程，避免中间 PID 被复用时带入旧的无关子进程；退出线程不假装贡献了完整区间的活动计数。
- 两段闲置样本不能证明长期无增长，也不能替代窗口销毁、真实业务负载或 macOS 验证。

API 口径参考 [GetThreadDescription](https://learn.microsoft.com/windows/win32/api/processthreadsapi/nf-processthreadsapi-getthreaddescription)、[QueryThreadCycleTime](https://learn.microsoft.com/windows/win32/api/realtimeapiset/nf-realtimeapiset-querythreadcycletime) 和 [Windows 内存性能信息](https://learn.microsoft.com/windows/win32/memory/memory-performance-information)。
