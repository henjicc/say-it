# 实时 SDK 会话事件等待的取消与音频尾包边界

## 原因与实现边界

内置 SDK 与 JavaScript 插件的实时 ASR 会话原先用 try_recv + sleep(10ms) 轮询音频，并在每轮后分发宿主事件。无消息时仍反复唤醒，音频、WebSocket 结果和定时器回调可能等待下一轮。这是识别会话的等待成本，不是没有识别任务时的托盘常驻成本。

SessionWait 在专属 JS 线程上复用一个 Waker，只轮询 AsrStreamReceiver::recv 和宿主 Notify；没有消息时 park，收尾时只等到实际截止时间。网络/TLS 仍在 Tokio 线程执行；不得将此等待器扩展为在 QuickJS 调用栈内运行任意网络 Future 的执行器。创建等待器的线程必须就是消费线程。

## 必须保留的语义

- HostEventSender 必须先入队，再 notify_one；Notify 和 unpark 的保留许可分别覆盖“事件到达但还未订阅”与“完成 poll 但还未 park”的竞态。
- 宿主每批最多取 MAX_EVENTS；达到批次上限时再次通知，避免通知合并后其余事件永久滞留。多一次空检查可以接受，重新建立固定频率轮询不可取。
- 宿主通知会放弃本次 recv future，因此磁盘音频票据和正在读取的块必须归接收器所有；不能把 pending 音频移进一个会被丢弃的临时 future。该边界已由既有 AsrStreamReceiver 实现保证。
- Stop、输入失败、发送端关闭、插件 namespace 停用都要唤醒。namespace 清理直接修改共享 AtomicBool，原来没有通道消息；现在所有权登记同时保留宿主通知的 Weak 引用，置取消标记后通知等待线程。
- 每次处理音频后仍分发宿主事件；不能因输入持续积压而饿死服务端结果。Finish 之后保留原来的 8 秒收尾期限，无消息时也必须准时报告超时；已经收到的 Stop 或输入错误仍优先于收尾期限，不能被改报成超时。

## 验证范围

回归覆盖无消息等待、通知早于订阅和竞态到达、音频/失败/Stop/断开/namespace 取消、超过内存阈值的音频与尾包、宿主定时器实际回调、超过 MAX_EVENTS 的分批有序消费。完整 Rust release 回归 648 项通过、21 项显式忽略、0 项失败。未调用真实麦克风或供应商，macOS 运行时未在本机验证。

性能入口为 scripts/measure-audio-performance.mjs 的 asr-idle-session / asr-idle-session-legacy，以及 asr-session-latency / asr-session-latency-legacy；每次独立进程。旧组冻结原有等待循环，未模拟 ASR 计算或网络，因此仅解释该等待环节。线程 cycles 来自 QueryThreadCycleTime，包含用户态和内核态；不能将其换算为墙钟时间，也不能把循环次数等同于操作系统全部上下文切换次数。[Microsoft 文档](https://learn.microsoft.com/en-us/windows/win32/api/realtimeapiset/nf-realtimeapiset-querythreadcycletime)。

上述第一阶段没有消除宿主 Promise 泵的 5ms 等待、网络取消检查的 25ms 定时检查或业务所需心跳。后续宿主等待改造见下文；两个阶段均不能外推为整应用 CPU 或功耗降幅。


## 本地对照结果

Windows release、每组 3 个独立进程取中位数，专属 JS 工作线程等待 2 秒且没有音频/宿主消息：旧循环 192 次，新路径一次等待、检查通道 2 次（进入等待与截止）。线程 CPU cycles 9,642,422 → 230,436，约减少 97.6%；进程峰值私有提交约 3.840 → 3.836 MiB，内存没有明显变化。线程 cycles 对比只覆盖此等待线程，不包含系统其他线程、网络取消检查或真实 ASR 计算。

另由本地生产者交替发送 60 个音频包和 60 个宿主通知，逐次确认接收，间隔在 0～10ms 内变化。各进程内先计算分位数，再对三个进程取中位数：

| 消息 | 旧 P50 | 新 P50 | 旧 P95 | 新 P95 |
| --- | ---: | ---: | ---: | ---: |
| 音频包 | 6.002 ms | 0.015 ms | 10.492 ms | 0.019 ms |
| 宿主通知 | 5.010 ms | 0.013 ms | 10.487 ms | 0.019 ms |

全部包和通知按预期收到；这不是麦克风采集到识别结果的端到端延迟。原始 JSON 为工作区 session-wake-asr-idle-session[-legacy].json 与 session-wake-asr-session-latency[-legacy].json，保存程序 SHA256。测量后只将产品中已无调用方的 try_recv 保留为测试接口，冻结旧轮询对照仍可重复运行。

## 宿主取消与截止时间的事件通知

移除定时检查前，必须先让取消状态可被订阅。裸 AtomicBool 的写入不会唤醒等待者；CancellationFlag 保留同步原子检查，并在写入后广播。等待方先注册 Notify，再检查状态，覆盖取消抢先到达和多个并发网络任务的竞态。ASR 的错误内容仍由 failure 锁保护，唤醒方不能在错误发布前把它误读成普通 Stop。

截止时间缩短时广播通知，延长时不通知；旧定时器到点后重新读取最新截止时间。音频包会频繁续期，若每次续期都唤醒网络任务，会用另一种无效唤醒替代轮询。不能直接在旧 timer 到点时报告超时。

网络取消等待改为取消通知与实际截止时间二选一。Promise 泵在执行完可运行 jobs 和宿主事件后等宿主通知；插件 sleep 等实际时长或取消，零时长直接返回。凭据读取保留四个有界后台工作线程，回复改为 oneshot，并同时等待取消和原有截止时间。取消终止宿主等待，不声称能中断已经进入系统凭据接口的阻塞调用。

所有异步等待仍通过 wait_for_host_io 交给 Tokio，QuickJS 线程只等结果；不在解释器调用栈运行网络 Future。析构继续使用独立取消标记和三秒预算；不能让业务取消跳过收尾。业务心跳和请求本身要求的定时器继续保留。

该阶段完整 release 回归 656 项通过、22 项显式忽略、0 项失败；覆盖订阅竞态、多等待方取消、截止时间缩短与续期、Promise/sleep 取消与超时、凭据等待取消，以及既有 HTTP、WebSocket、上传与析构测试。没有调用真实供应商，macOS 运行时仍未验证。

Windows release 下每组 3 个独立进程取中位数，冻结旧 25ms 网络检查与新等待在同一程序内对照：两秒等待的 Future poll 次数 78 → 2，等待线程 cycles 8,525,540 → 613,793（约减少 92.8%），峰值私有提交 3.750 → 3.762 MiB，内存没有明显改善。每进程 64 次取消通知，P50 22.005 → 0.014ms，P95 25.009 → 0.023ms。此测量不含实际网络、TLS 或识别计算，也不定量代表 Promise 泵和整应用收益。

用上一阶段程序与本阶段程序补测 300 秒等效采集数据：耗时中位数 52.852 → 51.116ms，峰值私有提交 3.988 → 3.984 MiB，全部输出哈希 f9c0d1d7d63e9fb9；在本地样本波动范围内未见明显回退。原始结果位于工作区/performance/host-control-host-network-{wait,cancel}[-legacy].json 与 host-control-capture-{session-wake,host-control}.json，均记录程序 SHA256。不能把等效音频数据的加速处理时间当成真实录音时长。
