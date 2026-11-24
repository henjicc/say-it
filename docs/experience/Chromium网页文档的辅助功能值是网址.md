# Chromium 网页文档的辅助功能「值」是网址

## 现象

在 Claude 桌面版、Edge、Chrome 这类 Chromium / Electron 应用里听写，软件上下文的「当前输入区域」经常是一串 `https://claude.ai/…` 网址，不是输入框里的文字。

## 原因

焦点落在网页文档本身、没有点进输入框时，文本探针（`context-probe/src/main.cpp`）读到的焦点元素就是这个网页文档：

- MSAA 的 `accValue`：网页文档（`ROLE_SYSTEM_DOCUMENT`）和链接（`ROLE_SYSTEM_LINK`）的值都是网址。网页文档的 `accName` 是页面标题，窗口标题里已经有了。
- UI Automation 的 `ValuePattern`：文档（`UIA_DocumentControlTypeId`）和超链接（`UIA_HyperlinkControlTypeId`）同样返回网址。

探针原来不看元素类型，把这个值当成了输入框里的文字。

## 做法

- 焦点元素是网页文档时，MSAA 这一级当作没读到，交给后面的读取方式；读不够时再截图识别补上。
- 是链接时只用名称（链接文字），不用值。
- UI Automation 读取时，文档和超链接不读 `ValuePattern`。

改探针后要重新运行 `npm run context-probe:build`，并用 `context-probe-x86_64-pc-windows-msvc.exe --self-test` 自检（退出码 0 为通过）。开发版运行时探针进程会占住可执行文件，要先停掉开发版。
