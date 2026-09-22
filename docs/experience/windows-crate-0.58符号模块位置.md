# windows crate 0.58 中鼠标与 D2D 符号的实际模块位置

给 Windows 原生窗口（native_indicator / native_orb）加鼠标交互时，`windows` crate 0.58 的
模块划分和直觉不一致，按头文件归属猜会编译失败（E0432 unresolved import）：

- `SetCapture` / `ReleaseCapture` / `TrackMouseEvent` / `TRACKMOUSEEVENT` / `TME_LEAVE`
  在 `Win32::UI::Input::KeyboardAndMouse`（特性 `Win32_UI_Input_KeyboardAndMouse`），
  不在 `UI::WindowsAndMessaging`。
- `WM_MOUSELEAVE` 在 `Win32::UI::Controls`，需要特性 `Win32_UI_Controls`。
  只为这一个常量不值得加整个特性，直接在代码里写 `const WM_MOUSELEAVE_MSG: u32 = 0x02A3;`
  （见 `src-tauri/src/desktop/native_orb.rs`）。
- `D2D1_ARC_SEGMENT` / `D2D1_ARC_SIZE_*` / `D2D1_SWEEP_DIRECTION_*` 在
  `Graphics::Direct2D`，而 `D2D1_FIGURE_BEGIN_*` / `D2D1_FIGURE_END_*` / `D2D_SIZE_F` /
  `D2D_POINT_2F` 在 `Graphics::Direct2D::Common`。

排查方法：直接在本机 registry 源码里定位符号——

```bash
grep -rl "\b符号名\b" ~/.cargo/registry/src/*/windows-0.58.0/src/Windows/Win32/
```
