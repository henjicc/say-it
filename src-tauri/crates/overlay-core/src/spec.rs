//! 一个原生窗口的声明：两个平台的宿主只凭它完成建窗、命令投递与动作回传。
use crate::canvas::TextEngine;
use crate::look::Look;
use crate::window::OverlayModel;

/// 模型的放置参考区（`Cx::area`）取自哪里。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placement {
    /// 跟随焦点屏并贴在底部。Windows：主屏工作区；macOS：焦点屏的 visibleFrame，
    /// dpi 取该屏缩放，宿主再按整屏水平居中并让开 Dock。
    FollowFocus,
    /// 以屏幕为参考区自行摆放。Windows：主屏全屏；macOS：焦点屏的 visibleFrame。
    Screen,
    /// 位置完全由命令决定，模型不读参考区。Windows 给空矩形，macOS 与 `Screen` 相同。
    Free,
}

pub trait OverlaySpec: 'static {
    /// 进程内唯一；Windows 用作窗口线程名和窗口类名。
    const NAME: &'static str;
    /// 窗口标题，只出现在系统的窗口列表里。
    const TITLE: &'static str;
    const PLACEMENT: Placement;
    /// 建窗时是否点击穿透（Windows 的扩展样式）。macOS 面板建好时一律穿透，
    /// 由模型输出 `SetInteractive(true)` 打开。
    const CLICK_THROUGH: bool;

    type Command: Send + 'static;
    type Action: Send + 'static;
    type Model<T: TextEngine + 'static>: OverlayModel<T, Command = Self::Command, Action = Self::Action>
        + 'static;

    /// 在窗口所属线程调用一次。Windows 传入的 look 固定为 `Look::WINDOWS_LEGACY`。
    fn model<T: TextEngine + 'static>(dpi: u32, look: &Look) -> Self::Model<T>;
    /// 应用动作出口；在窗口线程、主线程或渲染线程调用，必须立即返回。
    fn dispatch(action: Self::Action);
}
