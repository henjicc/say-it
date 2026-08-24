# macOS 全局鼠标释放事件与按钮状态不同步

## 现象

`CGEventTap` 能收到 `kCGEventLeftMouseUp`，但基于 `CGEventSourceButtonState` 组装的 `buttonDown` 仍可能是 `true`。如果识别器先判断持续按下状态，再判断释放沿，完整点击会被当作仍在按住，导致鼠标和触控板连击都无法成立。

## 处理原则

- 点击状态机应优先处理事件自身携带的 `leftPressed` / `leftReleased` 边沿，再参考全局按钮状态处理拖拽。
- macOS 原生回调应以当前 `mouseDown` / `mouseUp` 事件修正 `CGEventSourceButtonState` 的滞后一拍结果。
- 回归测试必须覆盖“收到释放沿，但组合按钮状态仍为按下”的样本，而不能只测试理想化的释放状态。
