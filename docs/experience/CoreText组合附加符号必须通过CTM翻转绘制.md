# CoreText 组合附加符号必须通过 CTM 翻转绘制

适用：左上原点、y 向下的 CoreGraphics 画布中调用 `CTLineDraw` 或 `CTRunDraw`。

翻转文字矩阵的 `d` 只能让普通字形看似正立，组合附加符号的纵向位置仍可能错误。CoreText 要求文字空间的 y 向上；附加符号包含相对基字形的定位，必须和整行一起变换。

正确做法：文字矩阵保持单位阵；在 save/restore 内平移到每行基线，再用 `scale(1, -1)` 局部翻转 CTM，以文字位置 `(0, 0)` 绘制。逐 run 着色也使用相同坐标，并在调用前重置文字矩阵与位置。恢复绘图状态后，后续图形继续使用原画布坐标。

回归用例：固定基线比较 `x`/`x́` 与 `a`/`ǎ` 的不透明像素纵向包围盒，附加符号应出现在基字形上方，底部不能明显向下扩展；同时比较分解形式 `ǎ` 与预组合 `ǎ`。覆盖 1x/2x 和整行/逐 run 着色两条路径，最后绘制矩形验证 CTM 恢复。

参考：[Apple CTRunDraw 文档](https://developer.apple.com/documentation/coretext/ctrundraw(_:_:_:))；当前 objc2-core-text 绑定的 `CTLine::draw` / `CTRun::draw` 注释也包含翻转文字矩阵可能错置 diacritics 的说明。
