//! 原生窗口共用的界面组件：窗口模型靠拼这些组件完成布局、绘制与指针交互。
//!
//! 模型在渲染线程排出各组件的矩形，把可交互区域登记进 `HitMap` 作为命中快照发布；
//! `UiPointer` 在指针所在线程只读这份快照，输出悬停、按下、点击与系统拖拽。
pub mod gesture;
pub mod hit;
pub mod layout;
pub mod paint;
pub mod pointer;

pub use hit::{HitKind, HitMap};
pub use pointer::{UiInput, UiPointer};
