# Field 操作按钮与控件高度不一致的根因与防线

## 现象

设置页「音频 → 提示音」里，`Field.actions` 中的「试听」按钮比左侧 Select 矮一截，两者底边不齐。
同类问题在实时字幕页头也出现过一次，当时是靠页面里手写 `[&>button]:h-10` 硬掰，反而掰错了。

## 根因

不是 flex 没对齐，而是**固定高度压过了 `items-stretch`**：

- `Input` / `Select` 只有一种高度：`--control-h`（44px），没有 sm 变体。
- `Button` / `IconButton` 的 `size="sm"` 会写死 `--control-h-sm`（34px），`size` 默认是 `md`。
- `Field` 的操作区容器写的是 `flex items-stretch`——它只表达了「希望等高」的意图，
  但子元素一旦自带固定 `height`，`items-stretch` 就完全失效。

于是「actions 里不能用 `size="sm"`」这条约束，实际上只存在于 CLAUDE.md 的文字里。
每写一个新页面就要重新记一次，记错了也不会报错、不会类型失败、构建照过，
**只有肉眼看截图才能发现**。这就是它反复出现的原因。

## 修复

把不变量从「作者要记住」改成「组件强制保证」——`ui/src/components/ui/Field.tsx`：

```tsx
const ACTIONS_CLASS = "flex shrink-0 items-stretch gap-2 [&>*]:h-[var(--control-h)]!";
```

`Field.actions` 语义上就是「紧挨着控件的操作」，只有一种合法高度，所以直接钉死是正确行为而非 hack。
两个 layout 分支（`stack` / `row`）共用同一常量。

注意 Tailwind v4 的 important 修饰符是**后缀** `h-[...]!`，不是 v3 的前缀 `!h-[...]`。
改完可在 `ui/dist/assets/index-*.css` 里 grep 验证是否真的生成了：

```
.\[\&\>\*\]\:h-\[var\(--control-h\)\]\!>*{height:var(--control-h)!important}
```

## 连带修正

`RealtimeSubtitlesPanel` 页头原有一处 `className="w-36 [&>button]:min-h-0 [&>button]:h-10 [&>button]:py-0"`，
注释写「与右侧两个 h-10 按钮等高对齐」。但那两个 Button 是默认 `md` = 44px，Select 原生也是 44px，
本来就是齐的；这个 hack 把 Select 压到 40px，**是它自己制造了错位**。已删除。

## 触发条件与正确做法

- 往 `Field.actions` 里放按钮时，**不要传 `size="sm"`**；组件现在会兜底，但代码应表达真实意图。
- 在页面里用 `[&>button]:h-*`、`items-end`、额外 `h-*` 去修标准控件的对齐，一律是错的信号：
  说明某个控件的尺寸来源不对，去改令牌或组件，不要在调用处打补丁。
- 新增带「控件 + 操作」的横排结构时，如果容器不是 `Field`，先确认该容器有没有同样的高度保证；
  没有就把保证加在容器组件里，而不是在页面里逐个对齐。
- 高密列表确需 34px 控件时，用 `--control-h-sm` 并把范围显式限定在列表内部。
