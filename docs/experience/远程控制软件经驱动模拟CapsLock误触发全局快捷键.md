# 远程控制软件经驱动模拟 CapsLock 误触发全局快捷键

## 现象

用手机经网易 UU 远程向电脑输入文字时，CapsLock 听写快捷键被反复触发，听写瞬间开始又结束，显示失败。

## 原因

远程控制软件在输入前会点击 CapsLock 同步两端大小写，并且是经驱动发送的：

- 不带 `LLKHF_INJECTED`，钩子里按"注入"过滤拦不住；
- 扫描码（CapsLock 为 0x3A）、`dwExtraInfo`（0）都和真实键盘一致；
- 唯一可区分的特征：按下到抬起的 `KBDLLHOOKSTRUCT.time` 差 0–1ms，并且常在数毫秒内连点两次。人手按键至少持续数十毫秒。

## 做法

`src-tauri/src/hotkey.rs` 中，快捷键按下后延迟 `MIN_HUMAN_PRESS_MS` 再生效；在这段时间内抬起的视为模拟点击：用按下序号作废待发信号，被吞掉的锁定键用 `tap_key` 还给系统。听写快捷键（短按、长按、配对短按）和字幕快捷键走同一套判定。

## 排查方法

在怀疑的场景下，挂一个只记录元数据的 `WH_KEYBOARD_LL` 探针（vk、scanCode、flags、dwExtraInfo、time，字母和数字打码），对比真实按键与可疑按键的差异，再决定过滤依据，不要按标志位猜测。
