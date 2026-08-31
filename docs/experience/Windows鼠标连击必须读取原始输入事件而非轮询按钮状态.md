# Windows 鼠标连击必须读取原始输入事件而非轮询按钮状态

## 根因

`WM_INPUT` 携带的是一条已排队的原始输入事件。若收到消息后只调用 `GetAsyncKeyState`，再用当前状态与上一次查询值比较来推断按下/释放，快速点击或消息排队时就会漏掉边沿：处理按下消息时，物理按钮可能已经松开，甚至已经进入下一次点击。

## 正确处理

- 在 `DefWindowProc` 清理前用 `GetRawInputData` 读取本条消息；先校验长度和 `RIM_TYPEMOUSE`，再读取 `RAWMOUSE.usButtonFlags`。
- 左键按下/释放来自输入包本身，不能由全局状态查询结果推断。按包顺序维护按钮按住状态，让纯移动样本仍能正确拒绝拖拽。
- 全局按钮查询只用于监听启动时初始化已按住的按钮；后续状态由原始事件更新。监听线程重建时使用独立状态。
- 同一包包含按下与释放时分别发出两个样本；不把滚轮、移动、右键等计为左键点击。
- 保留连击次数、时间间隔、拖拽限制与冷却规则，不靠放宽阈值补偿输入丢失。

回归覆盖原地 3～10 次点击、同包双沿、仅移动/滚轮、拖动后回到原处、无效输入包及冷却。鼠标实机验收还需覆盖确认/直接模式、后台其他应用中的输入框以及关闭手势后重新开启。

参考：[RAWMOUSE 按钮转换标记](https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-rawmouse)、[GetRawInputData](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getrawinputdata)。
