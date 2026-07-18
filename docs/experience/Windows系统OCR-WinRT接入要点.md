# Windows 系统 OCR（WinRT）接入要点

## 背景

`active_app_context/ocr.rs` 原本只有 PP-OCR（ocr-rs/MNN）一个引擎，用于「当前软件上下文」的窗口 OCR 提取方式。现新增 Windows 自带 OCR（`Windows.Media.Ocr`）作为可切换的第二引擎，默认使用系统 OCR。实现见 [windows_ocr.rs](../../src-tauri/src/active_app_context/windows_ocr.rs)。

## 关键 API 事实（windows-rs 0.58，已用本地 crate 源码核实，未在真实设备上跑通识别）

- **所需 Cargo feature**：`Media_Ocr`、`Graphics_Imaging`、`Storage_Streams`、`Foundation_Collections`、`Win32_System_WinRT`。`Media_Ocr` 隐含 `Media`→`Foundation`；`Graphics_Imaging` 隐含 `Graphics`→`Foundation`；`Storage_Streams` 隐含 `Storage`→`Foundation`。但 `Lines()`/`Words()` 返回的 `IVectorView<T>` 需要单独启用 `Foundation_Collections`，不会被上述任何一个自动带出。
- **`OcrEngine::TryCreateFromLanguage`/`TryCreateFromUserProfileLanguages()` 不需要 `Globalization` feature**（只有 `TryCreateFromLanguage` 才需要，因为它的参数类型是 `Language`）。若只用 `TryCreateFromUserProfileLanguages()`，可以不加 `Globalization`。
- **语言包缺失时的行为**：WinRT 的 `TryCreate*` 静态方法在系统里挑不出可用语言时，ABI 层返回的是「HRESULT 成功但输出指针为空」。windows-rs 的 `Type::from_abi`（`windows-core-0.58/src/type.rs`）对接口类型做了 `if !abi.is_null() { Ok(..) } else { Err(Error::empty()) }` 的转换，所以在 Rust 侧看到的是 **`Err`，不是 `Ok(null 指针)`**。不需要额外手动判空，直接 `map_err` 给出友好提示即可。
- **`OcrEngine::RecognizeAsync` 要求 `SoftwareBitmap`**：稳妥的构造路径是 `DataWriter::new()` → `WriteBytes(原始像素)` → `DetachBuffer()` 得到 `IBuffer`，再 `SoftwareBitmap::CreateCopyWithAlphaFromBuffer(buffer, format, width, height, alpha)`。不需要经过 `BitmapDecoder`/PNG 编解码这种异步往返，同步几行代码就能拿到位图。
- **像素格式必须是 `Bgra8`**（或 `Gray8`），`image` crate 产出的是 RGBA，需要手动交换 R/B 通道（`chunks_exact_mut(4)` 里 `swap(0, 2)`）；不要传 `Rgba8`，OCR 引擎不保证支持。
- **异步调用阻塞等待**：`IAsyncOperation<T>` 有 `.get()`，在 MTA 线程上可以同步阻塞直到完成，不需要引入消息循环或额外的 async 运行时桥接。
- **WinRT 线程初始化**：调用任何 WinRT API 前，当前线程需要 `RoInitialize(RO_INIT_MULTITHREADED)` 一次（对应 `windows::Win32::System::WinRT::RoInitialize`）。因为 OCR 识别本来就跑在专用常驻 worker 线程（`active-app-ocr`），用 `thread_local!` 存一个「是否已初始化」标记，在该线程第一次用到系统 OCR 时初始化一次即可，线程生命周期等同应用生命周期，不需要配对 `RoUninitialize`。

## 架构上的复用点

`ocr.rs` 里排序/去重/截断（`finalize_blocks`）和输出组装（`pipeline_output`）被抽成两套引擎共用的收尾函数；`OcrTextBlock`/`OcrPipelineOutput` 契约保持不变，调用方（`windows.rs::capture_and_recognize`）完全不用感知具体引擎，只需要把 `OcrEngineKind` 一路透传到 `ocr::run_full_window`。系统 OCR 没有常驻状态可缓存，worker 线程里只有 PP-OCR 分支保留 `Option<EngineState>` 生命周期管理。

## 未验证事项

以上事实均来自本地 `windows-rs` 源码核对与 `cargo check`/`cargo test` 编译期验证，**没有在真实 Windows 设备上实际触发过一次系统 OCR 识别**（依赖用户机器安装了对应语言的「光学字符识别」组件）。若识别结果异常或 `RecognizeAsync` 报错，先确认语言包是否安装，再检查 Bgra8 转换是否正确。
