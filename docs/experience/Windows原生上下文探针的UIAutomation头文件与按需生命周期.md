# Windows 原生上下文探针的 UI Automation 头文件与按需生命周期

## 适用场景

在独立 MSVC C++ 进程中使用 UI Automation、MSAA、IAccessible2，并由 Tauri 按需启动该进程时。

## UI Automation 头文件

启用 `WIN32_LEAN_AND_MEAN` 后，不能只包含 `UIAutomationClient.h`。该 MIDL 头依赖 COM 的 `interface` 等声明，缺少时会从头文件第一个前置声明开始产生大量“缺少类型说明符”错误。

稳定的包含顺序是：

```cpp
#include <windows.h>
#include <objbase.h>
#include <oleauto.h>
#include <UIAutomationClient.h>
```

链接至少包含 `UIAutomationCore`、`Oleacc`、`OleAut32`、`Ole32`、`User32`。使用 `CommandLineToArgvW` 时还需 `Shell32`。

## 生命周期边界

- 文本探针由 Rust 首次请求时启动，不随主程序启动常驻。
- 每次请求必须携带 `requestId`，响应不匹配时立即断开并重启探针。
- 跨进程读取超过硬截止时直接终止探针；不要让卡住的 COM 调用占用 Tauri 异步线程。
- OCR 引擎由单一工作线程持有，不能放入不可释放的进程级 `OnceLock<OcrEngine>`。
- “释放 OCR”必须记录为待处理状态，即使 OCR 工作线程尚未创建；否则用户在冷启动任务入队前切回文本模式，模型仍会在稍后加载并永久驻留。
- 黑名单和已确认的密码控件检查必须发生在窗口标题、正文、截图和剪贴板读取之前。
