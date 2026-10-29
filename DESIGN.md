---
name: "说吧！"
description: "克制、专业、高效的桌面语音工作台"
colors:
  accent: "#286EC8"
  frame: "#0E0F11"
  content: "#131417"
  panel: "#17181B"
  surface: "#1E1F23"
  surface-hover: "#24262A"
  surface-strong: "#2C2E33"
  foreground: "#ECEDEF"
  foreground-muted: "#A3A6AD"
  foreground-subtle: "#7E828A"
  foreground-faint: "#5C5F66"
  line: "#FFFFFF12"
  line-strong: "#FFFFFF1F"
  success: "#3FB27F"
  error: "#E5726C"
  warning: "#D9A441"
typography:
  page-title:
    fontFamily: "HarmonyOS Sans SC, MiSans, Source Han Sans SC, Noto Sans SC, system-ui, Segoe UI, Microsoft YaHei UI, sans-serif"
    fontSize: "18px"
    fontWeight: 600
  row:
    fontSize: "13px"
    fontWeight: 400
  value:
    fontSize: "14px"
    fontWeight: 400
  caption:
    fontSize: "12px"
    fontWeight: 400
rounded:
  sm: "6px"
  md: "8px"
  lg: "10px"
  xl: "12px"
spacing:
  group: "16px"
  inspector-x: "32px"
  list-column: "288px"
  rail: "68px"
  titlebar: "40px"
  row: "48px"
components:
  control:
    height: "32px"
    height-sm: "28px"
    rounded: "{rounded.md}"
---

# Design System: 说吧！

## Overview

**Creative North Star: "精密语音工作台"**

高频桌面生产力工具。界面克制、严肃：靠明度层级、细边框和排版建立秩序，不靠装饰。主窗口有两种形态，按窗口宽度自动切换：

- **简单模式**（窄窗口，默认约 440×400）：功能切换放在标题栏中部，设置是窗口按钮左侧的图标；每页只露出最必要的 3～5 项。
- **专业模式**（宽窗口，默认 1080×860，最大化一定是专业模式）：左侧 68px 图标导航，每页是「中栏列表 + 右侧检查器」。

两种模式共用同一套令牌、分组与行组件，切换时看起来是同一个软件变大或变小。

## Layers

从外到内一层比一层亮，层级色全部由用户背景色推导（`useThemeStore`）：

1. **外框**（标题栏、左侧导航）：最沉。
2. **内容层**：从外框里抬起，左上（简单模式为上方两角）10px 圆角，上边与左边一道细亮边；专业模式里是中栏列表的底色。
3. **检查器**：比内容层略亮，承载选中条目的设置。
4. **分组面**：`Group` 的底色，1px 细边框，顶部一道很淡的高光（`--shadow-raise`）。
5. **浮层**：下拉、提示和对话框，只在这里使用投影。

## Signature: 管线

处理流程（听写、字幕、转写）在专业模式中栏画成竖向管线：一根细线串起各个节点，节点前是 7px 圆点。圆点表达状态：灰色实心为普通，强调色为当前选中或已开启，空心圆为关闭。非流程页面（助手、模型与密钥、词库、设置）的中栏是普通分类列表，用线性图标，不画竖线。

## Colors

强调色只用于：开关打开、主按钮、当前选中节点的圆点、焦点环和链接。其他一律灰阶。状态色只表达状态，不做装饰。

## Typography

统一无衬线字体栈。层级只有几档：页面标题 18/600，节点值与正文 14，行名称与控件 13，说明与分组标题 12。等宽字体只用于代码与路径。

## Components

- **Group / Row**：设置的基本单元。Row 左侧名称、右侧控件，行高 48；说明只放 `hint`（悬停提示），错误与风险放 `message`（行下方可见）。
- **控件**：标准高度 32px，紧凑 28px，圆角 8px。下拉框放在 Row 右侧时给固定宽度；2～3 个选项的短选择用分段按钮（`Tabs variant="view"`）。
- **按钮**：主操作用强调色填充；普通操作是分组面底色 + 细边框；危险操作用错误色。
- **快捷键**：扁平展示，不做立体键帽。

## Native Overlays

听写胶囊、悬浮球、字幕条与主窗口同色系。胶囊固定宽度：录音只显示波形；处理时只显示「识别 / 处理 / 规则 / 输出」四段进度；完成时只闪一下对勾；只有出错才显示一句短提示。不显示时长、按键提示、字数或耗时。

## Do's and Don'ts

### Do

- 用明度层级、细边框和留白组织界面。
- 同一项设置只放在一个位置，其他地方用链接跳转。
- 让大面积展示真实内容（提示词、预览、结果），而不是说明文字。
- 保证键盘操作、可见焦点、标签关联和 WCAG AA 对比度。

### Don't

- 不用渐变、发光、拟物或装饰性玻璃效果（用户主动开启的系统毛玻璃除外）。
- 不用一张大卡片包住整页或整个检查器，不嵌套卡片。
- 不堆统计、编号、读数和说明文字。
- 页面内不使用页面级 Tab，不出现第三层导航。
