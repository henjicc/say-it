use crate::canvas::{Canvas, TextEngine};
use crate::geom::{Point, PxRect, Rect, Shape};
use crate::look::Look;
use std::time::{Duration, Instant};

pub const FRAME_INTERVAL: Duration = Duration::from_millis(16);
/// 帧定时器启动后第一帧的 dt（现状 FRAME_MS/1000）。
pub const FIRST_FRAME_DT: f32 = 0.016;

/// 本帧上屏参数：bounds（DesktopPx）已含入场/退场位移；alpha 为整窗不透明度，未量化。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameSpec {
    pub bounds: PxRect,
    pub dpi: u32,
    pub alpha: f32,
}

/// 模型要求宿主执行的副作用，按顺序执行；执行时宿主不得持有模型借用。
#[derive(Clone, Debug, PartialEq)]
pub enum WindowOp<A> {
    /// 在此位置画一帧：prepare_frame → draw → 上屏。隐藏后的第一次 Redraw 负责重新显示。
    Redraw,
    /// 立即隐藏（Win: mark_hidden + SW_HIDE；Mac: orderOut:）。
    Hide,
    /// 重新提到置顶层且不激活（Win: SetWindowPos TOPMOST；Mac: orderFrontRegardless）。
    RaiseTopmost,
    /// 移动并改尺寸；尺寸变化由宿主重建缓冲。
    SetBounds(PxRect),
    MoveTo {
        x: i32,
        y: i32,
    },
    /// false = 点击穿透（Win: WS_EX_TRANSPARENT；Mac: ignoresMouseEvents = YES）。
    SetInteractive(bool),
    /// Win: TrackMouseEvent(TME_LEAVE)；Mac: 空操作（追踪区常驻）。
    TrackMouseLeave,
    /// 进入系统拖拽（Win: ReleaseCapture + HTCAPTION 模态循环，返回后宿主补发 SystemDragEnded；
    /// Mac: performWindowDragWithEvent: 立即返回，宿主随即补发 CaptureLost，会话结束补发 SystemDragEnded）。
    /// 只会出现在指针层输出里。
    BeginSystemDrag,
    /// 取得键盘焦点并激活窗口（只对 `OverlaySpec::FOCUSABLE` 的窗口有效）。
    /// Win: SetForegroundWindow + SetFocus；Mac: makeKeyAndOrderFront。
    Focus,
    /// 应用级动作，宿主交给主 crate（不得阻塞）。
    Action(A),
}
pub type Ops<A> = Vec<WindowOp<A>>;

/// 窗口系统事件。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WindowEvent {
    /// 用户拖拽或系统移动了窗口（布局原点，DesktopPx）。Mac 只在拖拽会话中发送。
    Moved { x: i32, y: i32 },
    /// 窗口所在屏缩放变化；suggested 为新 dpi 下建议的矩形。
    DpiChanged { dpi: u32, suggested: PxRect },
    /// 系统拖拽结束，bounds 为最终布局矩形。
    SystemDragEnded { bounds: PxRect },
    /// 放置参考区（Cx.area）变化：显示器插拔、Dock 变化。Windows 宿主不发送。
    AreaChanged,
    /// 键盘焦点得失；只有 `OverlaySpec::FOCUSABLE` 的窗口会收到。
    Focus(bool),
}

/// 键盘修饰键。`command` 在 Windows 是 Ctrl，在 macOS 是 Command：快捷键按它判断。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub command: bool,
    pub shift: bool,
    pub alt: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Escape,
    Enter,
    Tab,
    Backspace,
    Delete,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    /// 与修饰键组合的字母键，大写 ASCII（快捷键用；文字输入走 `KeyInput::Text`）。
    Letter(char),
}

/// 有焦点窗口收到的键盘与输入法输入。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeyInput {
    Down { key: Key, modifiers: Modifiers },
    /// 确定输入的文字（键入字符或输入法上屏）。
    Text(String),
    /// 输入法组字中的文字；空串表示组字结束。caret 为组字串内的 UTF-16 光标位置。
    Compose { text: String, caret: u32 },
}

/// 指针输入。client 为相对布局原点的物理像素；screen 为 DesktopPx。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerEvent {
    Move {
        client: (i32, i32),
        screen: (i32, i32),
    },
    LeftDown {
        client: (i32, i32),
        screen: (i32, i32),
    },
    LeftUp {
        client: (i32, i32),
    },
    /// Mac：rightMouseUp:，以及 Control+左键抬起。
    RightUp {
        client: (i32, i32),
    },
    Leave,
    /// Win: WM_CAPTURECHANGED；Mac: performWindowDrag 之后。
    CaptureLost,
}

/// 指针层输出：窗口副作用，或转交渲染模型的输入。宿主按序执行；Model 立即交给
/// OverlayModel::input（Windows 同线程同步；Mac 经通道转给渲染线程）。
#[derive(Clone, Debug, PartialEq)]
pub enum PointerStep<A, I> {
    Op(WindowOp<A>),
    Model(I),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorShape {
    Arrow,
    Move,
    /// 可点击（胶囊智能处理中的「直接输入」）。
    Hand,
    /// 可选中的文字或输入框。
    Text,
}

/// 模型调用上下文：宿主每次处理消息取一次 now。
pub struct Cx<'a, T> {
    pub now: Instant,
    pub dpi: u32,
    /// 放置参考区（DesktopPx）。Windows：胶囊 = 主屏工作区，字幕 = 主屏全屏（现状）；
    /// macOS：两者都 = NSScreen.mainScreen.visibleFrame。悬浮球不用。
    pub area: PxRect,
    pub text: &'a T,
}

/// 帧定时器：语义与三处 sync_timer + last_frame 相同。由宿主持有。
#[derive(Debug, Default)]
pub struct FrameClock {
    next: Option<Instant>,
    last: Option<Instant>,
}
impl FrameClock {
    pub const fn new() -> Self {
        Self {
            next: None,
            last: None,
        }
    }
    pub fn is_active(&self) -> bool {
        self.next.is_some()
    }
    /// Some(true) = 需要启动（同时清帧间隔基准），Some(false) = 停止，None = 不变。
    pub fn sync(&mut self, want: bool) -> Option<bool> {
        if want && !self.is_active() {
            self.last = None;
            self.next = Some(Instant::now() + FRAME_INTERVAL);
            Some(true)
        } else if !want && self.is_active() {
            self.next = None;
            Some(false)
        } else {
            None
        }
    }
    /// 距上一帧的秒数；本轮第一帧返回 FIRST_FRAME_DT。
    pub fn delta(&mut self, now: Instant) -> f32 {
        if self.is_active() {
            self.next = Some(now + FRAME_INTERVAL);
        }
        self.last
            .replace(now)
            .map(|last| now.saturating_duration_since(last).as_secs_f32())
            .unwrap_or(FIRST_FRAME_DT)
    }
    /// 下一帧截止时间（Mac 渲染线程 recv_timeout 用）；未激活为 None。
    pub fn next_deadline(&self) -> Option<Instant> {
        self.next
    }
}

/// 指针层：按下、拖拽阈值、悬停、点击判定。只读命中快照，不持有渲染状态，
/// 可在与模型不同的线程运行（Mac 主线程）。
pub trait PointerModel: Default {
    /// 模型发布的命中快照（相位、交互几何、dpi 等），Copy 以便跨线程发布。
    type Hit: Copy + Send + Sync + 'static;
    type Input: Send + 'static;
    type Action: Send + 'static;
    fn event(
        &mut self,
        event: PointerEvent,
        hit: &Self::Hit,
    ) -> Vec<PointerStep<Self::Action, Self::Input>>;
    fn cursor(&self, hit: &Self::Hit) -> CursorShape {
        let _ = hit;
        CursorShape::Arrow
    }
    /// 输入法候选窗要贴着的光标矩形（DIP，布局坐标）；None = 当前没有输入框获得焦点。
    fn ime_rect(&self, hit: &Self::Hit) -> Option<Rect> {
        let _ = hit;
        None
    }
}

/// 一个悬浮窗的渲染状态机。T 为平台文本引擎；只在单线程使用。
pub trait OverlayModel<T: TextEngine> {
    type Command: Send + 'static;
    type Action: Send + 'static;
    type Pointer: PointerModel<Action = Self::Action>;

    fn apply(&mut self, command: Self::Command, cx: &Cx<'_, T>) -> Ops<Self::Action>;
    fn input(
        &mut self,
        input: <Self::Pointer as PointerModel>::Input,
        cx: &Cx<'_, T>,
    ) -> Ops<Self::Action>;
    fn window_event(&mut self, event: WindowEvent, cx: &Cx<'_, T>) -> Ops<Self::Action>;
    /// 键盘输入；只有 `OverlaySpec::FOCUSABLE` 且有焦点的窗口会收到。
    fn key(&mut self, input: KeyInput, cx: &Cx<'_, T>) -> Ops<Self::Action> {
        let _ = (input, cx);
        Vec::new()
    }
    /// 滚轮：point 为指针处的布局坐标（DIP），lines 为滚动的行数，向上为正。
    fn wheel(&mut self, point: Point, lines: f32, cx: &Cx<'_, T>) -> Ops<Self::Action> {
        let _ = (point, lines, cx);
        Vec::new()
    }
    /// 外观变化；通常返回 [Redraw]。Windows 宿主从不调用（模型以构造时的 Look 为准）。
    fn set_look(&mut self, look: &Look) -> Ops<Self::Action>;
    /// 一帧推进；dt 来自宿主 FrameClock。
    fn on_frame(&mut self, dt: f32, cx: &Cx<'_, T>) -> Ops<Self::Action>;
    /// 是否需要帧定时器（各窗口 sync_timer 的 want）。
    fn wants_frames(&self, now: Instant) -> bool;
    /// 可见时返回本帧上屏参数；允许更新只在上屏时生效的状态（悬浮球样式过渡目标、字幕布局）。
    fn prepare_frame(&mut self, cx: &Cx<'_, T>) -> Option<FrameSpec>;
    fn draw<C: Canvas<Text = T>>(&self, canvas: &mut C, now: Instant);
    /// 给指针层的命中快照；宿主在每次处理完命令/输入/帧后发布。
    fn hit(&self) -> <Self::Pointer as PointerModel>::Hit;
    /// 毛玻璃遮罩形状（DIP，布局坐标）；空 = 无玻璃。
    fn glass(&self) -> Vec<Shape> {
        Vec::new()
    }
    /// 需要收紧窗口时的内容外接矩形（DIP，布局坐标）；None = 整个布局窗口。只有 Mac 宿主使用。
    fn content_bounds(&self) -> Option<Rect> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frame_clock_starts_stops_and_resets_first_delta() {
        let mut clock = FrameClock::new();
        assert!(!clock.is_active());
        assert_eq!(clock.next_deadline(), None);
        assert_eq!(clock.sync(false), None);
        assert_eq!(clock.sync(true), Some(true));
        assert!(clock.is_active());
        let first_deadline = clock.next_deadline();
        assert!(first_deadline.is_some());
        assert_eq!(clock.sync(true), None);
        assert_eq!(clock.next_deadline(), first_deadline);
        let now = Instant::now();
        assert_eq!(clock.delta(now), FIRST_FRAME_DT);
        assert_eq!(clock.next_deadline(), Some(now + FRAME_INTERVAL));
        assert_eq!(clock.sync(true), None);
        assert_eq!(clock.delta(now + Duration::from_millis(32)), 0.032);
        assert_eq!(clock.sync(false), Some(false));
        assert_eq!(clock.next_deadline(), None);
        assert_eq!(clock.sync(true), Some(true));
        assert_eq!(clock.delta(now + Duration::from_secs(3)), FIRST_FRAME_DT);
    }

    #[test]
    fn frame_clock_saturates_backwards_time() {
        let mut clock = FrameClock::new();
        let now = Instant::now();
        clock.delta(now);
        assert_eq!(clock.delta(now - Duration::from_millis(1)), 0.0);
    }
}
