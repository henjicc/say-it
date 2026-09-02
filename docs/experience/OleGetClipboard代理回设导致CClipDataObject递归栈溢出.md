# OleGetClipboard 代理回设导致 CClipDataObject 递归栈溢出

## 现象与证据

应用空闲时仍可能以 `sayit-tokio-* has overflowed its stack` 退出。Windows 栈溢出日志经匹配当前系统 `ole32.pdb` 后，异常点为 `CClipDataObject::CacheDataPointer`，其后几十层均为 `CClipDataObject::GetData`。

这类现场与 QuickJS、HTTP 和 Tokio Future 无关。线程名称只表示调用恰好运行在 Tokio blocking worker，不能据此把 Tokio 当作递归源。

## 根因

`OleGetClipboard` 返回的是代表当前系统剪贴板的 `IDataObject`，不是拥有所有格式数据的快照。把该对象保存下来，在临时写入听写文本后再执行 `OleSetClipboard(saved)` 和 `OleFlushClipboard()`，会把系统剪贴板代理重新设为剪贴板数据源。系统读取格式时，代理最终再次读取自己，形成无界的 `CClipDataObject::GetData → GetData` 递归。

增加线程栈只能推迟递归耗尽栈，不能修复问题。

## 正确边界

- 禁止把 `OleGetClipboard` 的返回值用于后续 `OleSetClipboard` 恢复。
- 覆盖剪贴板前，用 `EnumClipboardFormats` 枚举实际格式，调用 `GetClipboardData` 取得源句柄，再用 `OleDuplicateData` 创建独立拥有的副本。
- 恢复时先确认剪贴板序列号仍是本次注入产生的序列号，避免覆盖用户随后复制的新内容；然后清空临时内容并逐格式通过 `SetClipboardData` 把副本所有权交给系统。
- 未转交的副本必须依据媒介类型使用 `ReleaseStgMedium` 释放，区分 HGLOBAL、GDI、METAFILEPICT 和 ENHMETAFILE，避免在取消恢复或部分失败时泄漏句柄。
- 粘贴按键发送失败时也要尝试恢复，只把原始粘贴错误返回给上层。

## 回归检查

- Windows 粘贴实现中不得重新出现 `OleGetClipboard`、`OleSetClipboard` 或 `OleFlushClipboard`。
- 文本、图片、文件列表、HTML/富文本等多个剪贴板格式应在粘贴后保持可用。
- 如果用户在注入完成与恢复之间复制了新内容，应用不得覆盖新剪贴板。
- 栈溢出日志中不应再出现连续的 `ole32!CClipDataObject::GetData` 帧。
