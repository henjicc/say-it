#![cfg(target_os = "macos")]
//! 原生悬浮窗的 macOS 后端：用 objc2 系 crate 驱动 `sayit-overlay-core` 的模型，
//! 负责 NSPanel 窗口、CoreGraphics/CoreText 绘制与输入转发。
//!
//! 只依赖 core 与 objc2 系 crate，不依赖 tauri 或主 crate，
//! 因此能在 Windows 上用 `npm run check:mac-overlay` 做 aarch64-apple-darwin 类型检查。

pub mod cg;
pub mod ct;
