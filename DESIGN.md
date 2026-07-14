---
name: "说吧！"
description: "克制、专业、高效的桌面语音工作台"
colors:
  accent: "#5199FF"
  accent-light: "#8EC1FF"
  accent-dark: "#1C6FEA"
  background: "#0A0E16"
  sidebar: "#080B12"
  overlay: "#12161F"
  foreground: "#FFFFFF"
  surface: "#FFFFFF09"
  line: "#FFFFFF14"
  success: "#25C36F"
  error: "#FF6B6B"
  warning: "#FFD166"
typography:
  headline:
    fontFamily: "HarmonyOS Sans SC, MiSans, Source Han Sans SC, Noto Sans SC, system-ui, Segoe UI, Microsoft YaHei UI, sans-serif"
    fontSize: "24px"
    fontWeight: 700
    lineHeight: 1.25
  title:
    fontFamily: "HarmonyOS Sans SC, MiSans, Source Han Sans SC, Noto Sans SC, system-ui, Segoe UI, Microsoft YaHei UI, sans-serif"
    fontSize: "15px"
    fontWeight: 600
    lineHeight: 1.5
  body:
    fontFamily: "HarmonyOS Sans SC, MiSans, Source Han Sans SC, Noto Sans SC, system-ui, Segoe UI, Microsoft YaHei UI, sans-serif"
    fontSize: "14px"
    fontWeight: 400
    lineHeight: 1.5
  label:
    fontFamily: "HarmonyOS Sans SC, MiSans, Source Han Sans SC, Noto Sans SC, system-ui, Segoe UI, Microsoft YaHei UI, sans-serif"
    fontSize: "12px"
    fontWeight: 500
    lineHeight: 1.5
rounded:
  sm: "6px"
  md: "10px"
  lg: "12px"
  xl: "16px"
spacing:
  field: "6px"
  group: "16px"
  grid: "24px"
  section: "32px"
components:
  button-primary:
    backgroundColor: "{colors.accent}"
    textColor: "{colors.foreground}"
    rounded: "{rounded.md}"
    height: "44px"
    padding: "0 16px"
  button-secondary:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.foreground}"
    rounded: "{rounded.md}"
    height: "44px"
    padding: "0 16px"
  input:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.foreground}"
    rounded: "{rounded.md}"
    height: "44px"
    padding: "0 16px"
---

# Design System: 说吧！

## Overview

**Creative North Star: "精密语音工作台"**

「说吧！」是高频桌面生产力工具。界面采用克制的深蓝黑分层、单一蓝色强调和紧凑表单密度，让用户始终知道当前状态、可执行操作和信息边界。视觉层级主要依赖字号、字重、留白和分隔线，不依赖装饰性容器。

整页或整个 Tab 禁止放进一张巨型卡片。复杂设置应拆成连续的扁平分区；只有独立实体、渐进展开内容或确有边界需求的列表可以使用带边框容器。

**Key Characteristics:**

- 深蓝黑背景与低对比度表面层级
- 44px 标准控件、34px 紧凑控件
- 扁平分区、清晰标题、克制分隔线
- 单一蓝色强调，状态色只表达状态
- 键盘友好、默认保护敏感信息

## Colors

背景接近黑色但保留蓝色倾向，白色文字通过透明度建立层级，蓝色只用于主要操作、选中态和焦点。

### Primary

- **工作台蓝**：主要按钮、选中 Tab、焦点和启用状态。
- **明亮工作台蓝**：强调链接和需要更高可见度的局部状态。

### Neutral

- **深蓝黑底**：主窗口内容背景。
- **侧栏黑**：标题栏和侧栏背景，必须比内容底更沉。
- **低亮表面**：输入框、折叠区块和独立实体，不用于包裹整个页面。
- **白色文字层级**：正文、次要说明和占位信息通过既有透明度令牌区分。

**The Restrained Accent Rule.** 蓝色只表达主操作、当前选择和焦点，不作为装饰填满页面。

## Typography

**Display Font:** 项目统一无衬线字体栈
**Body Font:** 项目统一无衬线字体栈
**Label/Mono Font:** Cascadia Mono、Consolas，仅用于模型名、代码和数值

**Character:** 中文与拉丁字符使用同一套克制、清晰的无衬线体系，通过字号和字重建立层级，不引入展示字体。

### Hierarchy

- **Headline**（700，24px，1.25）：页面标题。
- **Title**（600，15px，1.5）：分区标题和关键实体标题。
- **Body**（400，14px，1.5）：正文和控件文字，说明性长文限制在 75ch 内。
- **Label**（500，12px，1.5）：字段标签、提示和状态补充。

**The One Family Rule.** 产品界面只使用统一无衬线字体栈，等宽字体仅服务于代码和数据。

## Elevation

系统默认扁平，通过背景层级和边框表达结构。阴影只用于必须脱离文档流的下拉浮层与模态框，不给普通设置区块添加悬浮感。

### Shadow Vocabulary

- **Popover**（`0 18px 48px rgba(0,0,0,0.55)`）：下拉菜单和临时浮层。
- **Subtle**（`0 1px 2px rgba(0,0,0,0.35)`）：需要轻微分离的紧凑元素。

**The Flat-by-default Rule.** 普通页面分区无阴影；如果必须靠阴影才能看出分组，先重新检查结构和间距。

## Components

### Buttons

- **Shape:** 10px 圆角；标准高度 44px，紧凑高度 34px。
- **Primary:** 工作台蓝底、白字，只用于页面当前主要操作。
- **Secondary:** 低亮表面、细边框，用于普通操作。
- **Hover / Focus:** 使用既有强调色混合背景和 2px 可见焦点环。
- **Danger:** 只使用语义错误色，不硬编码页面私有红色。

### Cards / Containers

- **Corner Style:** 独立实体通常使用 12px 圆角。
- **Background:** 低亮表面；内部紧凑列表可使用主背景形成层级。
- **Border:** 1px `line` 令牌。
- **Usage:** 折叠供应商、独立预览和边界明确的列表可以使用；整页、整个 Tab 和普通单字段行禁止使用。

### Inputs / Fields

- **Style:** Input、Select 与同排 Button 标准高度统一为 44px，紧凑规格统一为 34px。
- **Actions:** 字段操作必须通过 `Field.actions` 布局；hint 独占控件下一行。
- **Focus:** 使用统一强调边框和可见焦点。
- **Secrets:** 持久化密钥使用 `SecretInput`，掩码只是展示状态，不进入输入值。

### Navigation

- Tabs 外框高度 44px，触发器高度 34px；当前项使用强调色，支持方向键、Home 和 End。

### Settings Sections

- 使用 `SettingsSection`、`SectionHeader`、`FormGrid` 和 `Field` 组织设置页。
- 同级分区间距 32px，区块内部默认间距 16px。

## Do's and Don'ts

### Do:

- **Do** 使用设计令牌和共享组件表达颜色、尺寸、圆角与交互态。
- **Do** 用扁平分区、标题、留白和分隔线组织复杂 Tab。
- **Do** 让相邻 Input、Select 和 Button 使用相同的 44px 或 34px 尺寸契约。
- **Do** 让持久化密钥默认打码，仅在用户明确点击后短暂读取明文。
- **Do** 保证键盘操作、可见焦点、标签关联和 WCAG AA 对比度。

### Don't:

- **Don't** 用一张巨型卡片包住整个页面或 Tab，再继续嵌套卡片。
- **Don't** 让相邻输入框、下拉框和按钮高度或基线不一致。
- **Don't** 让同类设置拥有不同的密钥掩码、显隐或保存语义。
- **Don't** 用页面级硬编码高度、颜色或间距修补共享组件缺陷。
- **Don't** 使用渐变文字、装饰性玻璃效果、无意义动效或高饱和色块。
