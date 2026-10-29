# 测试用 yield_now 忙等在 macOS 上饿死后台读写线程导致超时

## 现象

`commands::asr::session_wait::tests::host_notifications_do_not_drop_spilled_audio_or_finish_tail` 在 Windows 本机 0.3 秒通过，macOS CI 上 5 秒截止时间内只收到 613/1025 个包即超时（多次重跑基本必现）。

## 根因

测试每轮循环都 `host.notify_one()`，`SessionWait::wait` 在输入未就绪时总能立刻返回 `HostEvents`，等待者从不休眠，只靠 `std::thread::yield_now()` 让出 CPU。落盘音频要靠暂存写入线程和 `spawn_blocking` 读盘线程推进；macOS 的 yield 不保证把 CPU 交给这些刚被唤醒的线程，CI 并行跑测试时每个包都要多等一个调度周期。产品代码在无事件时会 park，不受影响，问题只在测试的忙等写法。

## 做法

- 验证“被宿主通知打断不丢数据”时，每个数据单元至多打断一次，打断后让等待者正常休眠，由真实唤醒继续；同时断言打断确实发生过，避免测试退化成不打断。
- 测试里不要用 `yield_now` 忙等其他线程推进；需要等待就用真实的阻塞/唤醒路径。
- 截止时间是防挂死的兜底，超时先查是否忙等或唤醒丢失，不要直接加大截止时间。
