//! 悬浮球设置面板：右键悬浮球弹出，贴在球的一侧。调大小、不透明度与自动回车，
//! 以及打开主界面、恢复显示默认值、关闭悬浮球。点别处（失去焦点）或按 Esc 收起。
//!
//! 设置值由后端快照（`OrbMenuView`）决定；这里只保留悬停、按下、拖动中的滑块与键盘焦点。
//! 滑块拖动时先在本地更新数值并逐次发出动作，拖动期间到达的快照不覆盖正在拖的那一项。
use crate::anim::Transition;
use crate::canvas::{Canvas, FontFamily, ParagraphAlign, SvgIcon, TextAlign, TextEngine, TextStyle};
use crate::geom::{Color, Point, PxRect, Rect, Shape, Stroke, Transform};
use crate::look::{Look, Profile};
use crate::ui::{paint, HitKind, HitMap, UiInput, UiPointer};
use crate::window::*;
use std::time::Instant;

const WIDTH: f32 = 280.0;
const PAD: f32 = 14.0;
const RADIUS: f32 = 12.0;
const HEADER: f32 = 26.0;
const GAP: f32 = 12.0;
const LABEL_H: f32 = 18.0;
const LABEL_GAP: f32 = 6.0;
const SLIDER_H: f32 = 20.0;
const TRACK_H: f32 = 4.0;
const THUMB: f32 = 14.0;
const TOGGLE_H: f32 = 36.0;
const SWITCH_W: f32 = 36.0;
const SWITCH_H: f32 = 20.0;
const ACTION_H: f32 = 36.0;
const ACTION_GAP: f32 = 2.0;
const DIVIDER_PAD: f32 = 10.0;
const ERROR_GAP: f32 = 8.0;
const ICON: f32 = 16.0;
/// 面板与悬浮球之间的距离（DIP）。
const ORB_GAP: f32 = 8.0;
/// 滑块的大步长（PageUp / PageDown）。
const PAGE_STEP: i32 = 10;

const ID_CLOSE: u16 = 1;
const ID_SIZE: u16 = 2;
const ID_OPACITY: u16 = 3;
const ID_AUTO_ENTER: u16 = 4;
const ID_OPEN_MAIN: u16 = 5;
const ID_RESET: u16 = 6;
const ID_DISABLE: u16 = 7;
/// Tab 键的焦点顺序。
const FOCUS_ORDER: [u16; 7] = [
    ID_CLOSE,
    ID_SIZE,
    ID_OPACITY,
    ID_AUTO_ENTER,
    ID_OPEN_MAIN,
    ID_RESET,
    ID_DISABLE,
];

// Lucide 图标，24×24 视框。
const CLOSE_ICON: SvgIcon = SvgIcon {
    key: "orb-menu-close",
    d: "M18 6L6 18M6 6l12 12",
};
const OPEN_MAIN_ICON: SvgIcon = SvgIcon {
    key: "orb-menu-open-main",
    d: "M5 3h14a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2zM3 9h18M15 14l-3-3l-3 3",
};
const RESET_ICON: SvgIcon = SvgIcon {
    key: "orb-menu-reset",
    d: "M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8M3 3v5h5",
};
const POWER_ICON: SvgIcon = SvgIcon {
    key: "orb-menu-power",
    d: "M12 2v10M18.4 6.6a9 9 0 1 1-12.77.04",
};

/// 后端给的设置快照。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrbMenuView {
    /// 十分之一百分比，45 表示 4.5%。
    pub size_percent: u16,
    pub size_range: (u16, u16),
    pub opacity: u8,
    pub opacity_range: (u8, u8),
    pub auto_enter: bool,
    /// 最近一次操作失败的原因。
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    /// 弹出面板。orb 为悬浮球矩形，work 为它所在屏的工作区（都是 DesktopPx）。
    Show {
        view: OrbMenuView,
        orb: PxRect,
        work: PxRect,
        focus: bool,
    },
    Update(OrbMenuView),
    Hide,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OrbMenuAction {
    SetSize(u16),
    SetOpacity(u8),
    SetAutoEnter(bool),
    /// 大小与不透明度恢复默认值。
    ResetDisplay,
    OpenMainWindow,
    DisableOrb,
    Close,
    /// 键盘焦点得失；失焦时后端收起面板。
    Focused(bool),
}

/// 面板放在悬浮球右侧，右侧放不下改到左侧；纵向与球居中。最后都夹进工作区。
/// 所有量都是 DesktopPx；工作区为空时直接放在球右侧。
pub fn place(orb: PxRect, size: (i32, i32), gap: i32, work: PxRect) -> (i32, i32) {
    let right_x = orb.right() + gap;
    if work.width <= 0 || work.height <= 0 {
        return (right_x, orb.y);
    }
    let x = if right_x + size.0 <= work.right() {
        right_x
    } else {
        orb.x - size.0 - gap
    };
    let y = orb.y + orb.height / 2 - size.1 / 2;
    let max_x = (work.right() - size.0).max(work.x);
    let max_y = (work.bottom() - size.1).max(work.y);
    (x.clamp(work.x, max_x), y.clamp(work.y, max_y))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Slider {
    Size,
    Opacity,
}

/// 各部分的位置（DIP，相对面板左上角）。
struct Rows {
    close: Rect,
    size_label: Rect,
    size: Rect,
    opacity_label: Rect,
    opacity: Rect,
    toggle: Rect,
    divider: f32,
    actions: [Rect; 3],
    error: Rect,
}

fn rows() -> Rows {
    let inner = Rect::new(PAD, PAD, WIDTH - PAD, 0.0);
    let close = Rect::new(WIDTH - 10.0 - HEADER, PAD, WIDTH - 10.0, PAD + HEADER);
    let mut y = PAD + HEADER + GAP;
    let field = |y: &mut f32| {
        let label = Rect::new(inner.left, *y, inner.right, *y + LABEL_H);
        let top = *y + LABEL_H + LABEL_GAP;
        let slider = Rect::new(inner.left, top, inner.right, top + SLIDER_H);
        *y = slider.bottom + GAP;
        (label, slider)
    };
    let (size_label, size) = field(&mut y);
    let (opacity_label, opacity) = field(&mut y);
    let toggle = Rect::new(inner.left, y, inner.right, y + TOGGLE_H);
    y = toggle.bottom + GAP;
    let divider = y;
    y += DIVIDER_PAD;
    let mut actions = [Rect::ZERO; 3];
    for action in &mut actions {
        *action = Rect::new(inner.left - 6.0, y, inner.right + 6.0, y + ACTION_H);
        y += ACTION_H + ACTION_GAP;
    }
    let error = Rect::new(inner.left, y - ACTION_GAP + ERROR_GAP, inner.right, 0.0);
    Rows {
        close,
        size_label,
        size,
        opacity_label,
        opacity,
        toggle,
        divider,
        actions,
        error,
    }
}

/// 深色工作台配色（与主窗口令牌一致）。
mod palette {
    pub const PANEL: (f32, f32, f32) = (38.0 / 255.0, 38.0 / 255.0, 38.0 / 255.0);
    pub const FG: (f32, f32, f32) = (236.0 / 255.0, 237.0 / 255.0, 239.0 / 255.0);
    pub const MUTED: (f32, f32, f32) = (180.0 / 255.0, 183.0 / 255.0, 189.0 / 255.0);
    pub const SUBTLE: (f32, f32, f32) = (152.0 / 255.0, 156.0 / 255.0, 164.0 / 255.0);
    pub const TRACK: (f32, f32, f32) = (58.0 / 255.0, 58.0 / 255.0, 58.0 / 255.0);
    pub const SWITCH_OFF: (f32, f32, f32) = (74.0 / 255.0, 74.0 / 255.0, 74.0 / 255.0);
    pub const ACCENT: (f32, f32, f32) = (39.0 / 255.0, 103.0 / 255.0, 234.0 / 255.0);
    pub const ACCENT_TEXT: (f32, f32, f32) = (107.0 / 255.0, 152.0 / 255.0, 241.0 / 255.0);
    pub const ERR: (f32, f32, f32) = (229.0 / 255.0, 114.0 / 255.0, 108.0 / 255.0);
    pub const WHITE: (f32, f32, f32) = (1.0, 1.0, 1.0);
}

fn text_style(size: f32, weight: u16, wrap: bool) -> TextStyle<'static> {
    TextStyle {
        family: FontFamily::SystemUi,
        size,
        weight,
        align: TextAlign::Leading,
        paragraph: if wrap { ParagraphAlign::Near } else { ParagraphAlign::Center },
        wrap,
        line_spacing: None,
    }
}

pub struct OrbMenu {
    look: Look,
    dpi: u32,
    visible: bool,
    transition: Transition,
    view: OrbMenuView,
    orb: PxRect,
    work: PxRect,
    /// 错误文字排版后的高度（DIP），没有错误时为 0。
    error_height: f32,
    hover: Option<u16>,
    pressed: Option<u16>,
    dragging: Option<Slider>,
    /// 键盘焦点所在的控件；只有用 Tab 移动过才画焦点框。
    focus: Option<u16>,
    keyboard: bool,
}

impl OrbMenu {
    pub fn new(dpi: u32, look: &Look) -> Self {
        Self {
            look: *look,
            dpi: dpi.max(1),
            visible: false,
            transition: Transition::new(false),
            view: OrbMenuView {
                size_percent: 0,
                size_range: (0, 1),
                opacity: 0,
                opacity_range: (0, 1),
                auto_enter: false,
                error: None,
            },
            orb: PxRect::default(),
            work: PxRect::default(),
            error_height: 0.0,
            hover: None,
            pressed: None,
            dragging: None,
            focus: None,
            keyboard: false,
        }
    }
    fn scale(&self) -> f32 {
        self.dpi as f32 / 96.0
    }
    /// 画刷颜色一律 straight alpha，不沿用胶囊与悬浮球在 Windows 上的 rgb×a 兼容规则。
    fn color(&self, rgb: (f32, f32, f32), alpha: f32) -> Color {
        Color::new(rgb.0, rgb.1, rgb.2, alpha)
    }
    fn height(&self) -> f32 {
        let error = if self.error_height > 0.0 {
            self.error_height + ERROR_GAP - ACTION_GAP
        } else {
            -ACTION_GAP
        };
        rows().actions[2].bottom + ACTION_GAP + error + PAD
    }
    fn bounds(&self) -> PxRect {
        let scale = self.scale();
        let size = (
            (WIDTH * scale).round() as i32,
            (self.height() * scale).round() as i32,
        );
        let (x, y) = place(self.orb, size, (ORB_GAP * scale).round() as i32, self.work);
        PxRect::new(x, y, size.0, size.1)
    }
    fn measure_error<T: TextEngine>(&mut self, engine: &T) {
        self.error_height = self
            .view
            .error
            .as_deref()
            .and_then(|error| {
                engine.layout(error, &text_style(12.0, 400, true), WIDTH - PAD * 2.0, f32::MAX)
            })
            .map_or(0.0, |layout| engine.metrics(&layout).height);
    }
    /// 换上新快照；正在拖的滑块保留本地数值。
    fn take_view<T: TextEngine>(&mut self, mut view: OrbMenuView, engine: &T, ops: &mut Ops<OrbMenuAction>) {
        match self.dragging {
            Some(Slider::Size) => view.size_percent = self.view.size_percent,
            Some(Slider::Opacity) => view.opacity = self.view.opacity,
            None => {}
        }
        let before = self.bounds();
        self.view = view;
        self.measure_error(engine);
        if self.visible && before != self.bounds() {
            ops.push(WindowOp::SetBounds(self.bounds()));
        }
        ops.push(WindowOp::Redraw);
    }

    fn slider_rect(slider: Slider) -> Rect {
        let rows = rows();
        match slider {
            Slider::Size => rows.size,
            Slider::Opacity => rows.opacity,
        }
    }
    fn slider_of(id: u16) -> Option<Slider> {
        match id {
            ID_SIZE => Some(Slider::Size),
            ID_OPACITY => Some(Slider::Opacity),
            _ => None,
        }
    }
    fn range(&self, slider: Slider) -> (i32, i32, i32) {
        let v = &self.view;
        match slider {
            Slider::Size => (
                i32::from(v.size_percent),
                i32::from(v.size_range.0),
                i32::from(v.size_range.1),
            ),
            Slider::Opacity => (
                i32::from(v.opacity),
                i32::from(v.opacity_range.0),
                i32::from(v.opacity_range.1),
            ),
        }
    }
    /// 滑轨两端各缩进半个滑钮，数值 0..1 对应滑钮圆心的行程。
    fn track(slider: Slider) -> (f32, f32) {
        let rect = Self::slider_rect(slider);
        (rect.left + THUMB / 2.0, rect.right - THUMB / 2.0)
    }
    fn fraction(&self, slider: Slider) -> f32 {
        let (value, min, max) = self.range(slider);
        if max <= min {
            return 0.0;
        }
        (value - min) as f32 / (max - min) as f32
    }
    /// 改滑块数值；数值真的变了才发动作。
    fn set_value(&mut self, slider: Slider, value: i32, ops: &mut Ops<OrbMenuAction>) {
        let (current, min, max) = self.range(slider);
        let value = value.clamp(min, max);
        if value == current {
            return;
        }
        match slider {
            Slider::Size => {
                self.view.size_percent = value as u16;
                ops.push(WindowOp::Action(OrbMenuAction::SetSize(value as u16)));
            }
            Slider::Opacity => {
                self.view.opacity = value as u8;
                ops.push(WindowOp::Action(OrbMenuAction::SetOpacity(value as u8)));
            }
        }
        ops.push(WindowOp::Redraw);
    }
    fn slide_to(&mut self, slider: Slider, x: f32, ops: &mut Ops<OrbMenuAction>) {
        let (_, min, max) = self.range(slider);
        let (left, right) = Self::track(slider);
        let t = ((x - left) / (right - left)).clamp(0.0, 1.0);
        self.set_value(slider, min + (t * (max - min) as f32).round() as i32, ops);
    }
    fn step(&mut self, slider: Slider, delta: i32, ops: &mut Ops<OrbMenuAction>) {
        let (value, _, _) = self.range(slider);
        self.set_value(slider, value + delta, ops);
    }

    fn click(&mut self, id: u16, ops: &mut Ops<OrbMenuAction>) {
        let action = match id {
            ID_CLOSE => OrbMenuAction::Close,
            ID_AUTO_ENTER => {
                self.view.auto_enter = !self.view.auto_enter;
                ops.push(WindowOp::Redraw);
                OrbMenuAction::SetAutoEnter(self.view.auto_enter)
            }
            ID_OPEN_MAIN => OrbMenuAction::OpenMainWindow,
            ID_RESET => OrbMenuAction::ResetDisplay,
            ID_DISABLE => OrbMenuAction::DisableOrb,
            _ => return,
        };
        ops.push(WindowOp::Action(action));
    }

    fn focus_rect(id: u16) -> Rect {
        let rows = rows();
        match id {
            ID_CLOSE => rows.close,
            ID_SIZE => rows.size,
            ID_OPACITY => rows.opacity,
            ID_AUTO_ENTER => rows.toggle,
            ID_OPEN_MAIN => rows.actions[0],
            ID_RESET => rows.actions[1],
            _ => rows.actions[2],
        }
    }

    fn draw_icon<C: Canvas>(canvas: &mut C, icon: &SvgIcon, center: Point, color: Color) {
        canvas.save();
        canvas.concat_transform(Transform {
            scale: ICON / 24.0,
            dx: center.x - ICON / 2.0,
            dy: center.y - ICON / 2.0,
        });
        canvas.stroke_icon(icon, color, Stroke::round(2.0));
        canvas.restore();
    }
    fn backdrop<C: Canvas>(&self, canvas: &mut C, id: u16, rect: Rect) {
        let alpha = match (self.pressed == Some(id), self.hover == Some(id)) {
            (true, _) => 0.14,
            (false, true) => 0.08,
            _ => return,
        };
        canvas.fill_rounded_rect(rect, 8.0, self.color(palette::WHITE, alpha));
    }
    /// 在 rect 内靠右画一行文字。
    fn trailing_label<C: Canvas>(canvas: &mut C, text: &str, style: &TextStyle<'_>, rect: Rect, color: Color) {
        let engine = canvas.text();
        let Some(layout) = engine.layout(text, style, rect.width(), rect.height()) else {
            return;
        };
        let width = engine.metrics(&layout).width;
        canvas.draw_text(&layout, Point::new(rect.right - width, rect.top), color, None);
    }
    fn draw_slider<C: Canvas>(&self, canvas: &mut C, slider: Slider, label: Rect, name: &str, value: &str) {
        paint::label(canvas, name, &text_style(12.0, 400, false), label, self.color(palette::MUTED, 1.0));
        Self::trailing_label(canvas, value, &text_style(12.0, 400, false), label, self.color(palette::SUBTLE, 1.0));
        let rect = Self::slider_rect(slider);
        let (left, right) = Self::track(slider);
        let cy = (rect.top + rect.bottom) / 2.0;
        let x = left + (right - left) * self.fraction(slider);
        let track = Rect::new(rect.left, cy - TRACK_H / 2.0, rect.right, cy + TRACK_H / 2.0);
        canvas.fill_rounded_rect(track, TRACK_H / 2.0, self.color(palette::TRACK, 1.0));
        let filled = Rect::new(track.left, track.top, x.max(track.left + TRACK_H), track.bottom);
        canvas.fill_rounded_rect(filled, TRACK_H / 2.0, self.color(palette::ACCENT, 1.0));
        let radius = if self.dragging == Some(slider) { THUMB / 2.0 + 1.0 } else { THUMB / 2.0 };
        canvas.fill_circle(Point::new(x, cy), radius, self.color(palette::FG, 1.0));
    }
    fn draw_toggle<C: Canvas>(&self, canvas: &mut C, rect: Rect) {
        let text = Rect::new(rect.left, rect.top, rect.right - SWITCH_W - 10.0, rect.bottom);
        let mid = (rect.top + rect.bottom) / 2.0;
        paint::label(
            canvas,
            "自动回车",
            &text_style(13.0, 400, false),
            Rect::new(text.left, rect.top, text.right, mid + 1.0),
            self.color(palette::FG, 1.0),
        );
        paint::label(
            canvas,
            "识别完成后自动发送",
            &text_style(11.0, 400, false),
            Rect::new(text.left, mid + 1.0, text.right, rect.bottom),
            self.color(palette::SUBTLE, 1.0),
        );
        let on = self.view.auto_enter;
        let track = Rect::new(
            rect.right - SWITCH_W,
            mid - SWITCH_H / 2.0,
            rect.right,
            mid + SWITCH_H / 2.0,
        );
        let fill = if on { palette::ACCENT } else { palette::SWITCH_OFF };
        canvas.fill_rounded_rect(track, SWITCH_H / 2.0, self.color(fill, 1.0));
        let knob_x = if on { track.right - SWITCH_H / 2.0 } else { track.left + SWITCH_H / 2.0 };
        canvas.fill_circle(Point::new(knob_x, mid), SWITCH_H / 2.0 - 2.0, self.color(palette::WHITE, 1.0));
    }
    fn draw_action<C: Canvas>(&self, canvas: &mut C, id: u16, rect: Rect, icon: &SvgIcon, text: &str, danger: bool) {
        self.backdrop(canvas, id, rect);
        let (icon_color, text_color) = if danger {
            (palette::ERR, palette::ERR)
        } else if self.hover == Some(id) {
            (palette::ACCENT_TEXT, palette::FG)
        } else {
            (palette::ACCENT_TEXT, palette::MUTED)
        };
        let mid = (rect.top + rect.bottom) / 2.0;
        Self::draw_icon(canvas, icon, Point::new(rect.left + 8.0 + ICON / 2.0, mid), self.color(icon_color, 1.0));
        paint::label(
            canvas,
            text,
            &text_style(13.0, 400, false),
            Rect::new(rect.left + 8.0 + ICON + 10.0, rect.top, rect.right - 8.0, rect.bottom),
            self.color(text_color, 1.0),
        );
    }
}

impl<T: TextEngine> OverlayModel<T> for OrbMenu {
    type Command = Command;
    type Action = OrbMenuAction;
    type Pointer = UiPointer<OrbMenuAction>;

    fn apply(&mut self, command: Command, cx: &Cx<'_, T>) -> Ops<OrbMenuAction> {
        if cx.dpi != 0 {
            self.dpi = cx.dpi;
        }
        let mut ops = Vec::new();
        match command {
            Command::Show {
                view,
                orb,
                work,
                focus,
            } => {
                if !self.visible {
                    self.focus = None;
                    self.keyboard = false;
                }
                self.orb = orb;
                self.work = work;
                self.dragging = None;
                let mut sink = Vec::new();
                self.take_view(view, cx.text, &mut sink);
                self.visible = true;
                self.transition.show();
                ops.extend([
                    WindowOp::SetBounds(self.bounds()),
                    WindowOp::SetInteractive(true),
                    WindowOp::Redraw,
                    WindowOp::RaiseTopmost,
                ]);
                if focus {
                    ops.push(WindowOp::Focus);
                }
            }
            Command::Update(view) => self.take_view(view, cx.text, &mut ops),
            Command::Hide => {
                self.hover = None;
                self.pressed = None;
                self.dragging = None;
                ops = if self.transition.hide() {
                    vec![WindowOp::SetInteractive(false)]
                } else {
                    self.visible = false;
                    vec![WindowOp::Hide]
                };
            }
        }
        ops
    }

    fn input(&mut self, input: UiInput, _: &Cx<'_, T>) -> Ops<OrbMenuAction> {
        let mut ops = Vec::new();
        match input {
            UiInput::Hover(id) => self.hover = id,
            UiInput::Press(id) => self.pressed = id,
            UiInput::Click(id) => self.click(id, &mut ops),
            UiInput::RightClick(_) => return ops,
            UiInput::PointPress { id, point } => {
                if let Some(slider) = Self::slider_of(id) {
                    self.dragging = Some(slider);
                    self.focus = Some(id);
                    self.keyboard = false;
                    self.slide_to(slider, point.x, &mut ops);
                }
            }
            UiInput::PointDrag { point, .. } => {
                if let Some(slider) = self.dragging {
                    self.slide_to(slider, point.x, &mut ops);
                }
                return ops;
            }
            UiInput::PointRelease { .. } => self.dragging = None,
        }
        ops.push(WindowOp::Redraw);
        ops
    }

    fn key(&mut self, input: KeyInput, _: &Cx<'_, T>) -> Ops<OrbMenuAction> {
        let mut ops = Vec::new();
        let activate = match &input {
            KeyInput::Down { key: Key::Escape, .. } => {
                ops.push(WindowOp::Action(OrbMenuAction::Close));
                return ops;
            }
            KeyInput::Down { key: Key::Tab, modifiers } => {
                let len = FOCUS_ORDER.len();
                let index = self
                    .focus
                    .and_then(|id| FOCUS_ORDER.iter().position(|item| *item == id));
                let next = match (index, modifiers.shift) {
                    (None, false) => 0,
                    (None, true) => len - 1,
                    (Some(i), false) => (i + 1) % len,
                    (Some(i), true) => (i + len - 1) % len,
                };
                self.focus = Some(FOCUS_ORDER[next]);
                self.keyboard = true;
                ops.push(WindowOp::Redraw);
                return ops;
            }
            KeyInput::Down { key: Key::Enter, .. } => true,
            KeyInput::Text(text) => text == " ",
            KeyInput::Down { key, .. } => {
                let Some(slider) = self.focus.and_then(Self::slider_of) else {
                    return ops;
                };
                let (value, min, max) = self.range(slider);
                let target = match key {
                    Key::Left | Key::Down => value - 1,
                    Key::Right | Key::Up => value + 1,
                    Key::PageDown => value - PAGE_STEP,
                    Key::PageUp => value + PAGE_STEP,
                    Key::Home => min,
                    Key::End => max,
                    _ => return ops,
                };
                self.keyboard = true;
                self.set_value(slider, target, &mut ops);
                return ops;
            }
            KeyInput::Compose { .. } => false,
        };
        if activate {
            if let Some(id) = self.focus {
                self.click(id, &mut ops);
            }
        }
        ops
    }

    fn wheel(&mut self, point: Point, lines: f32, _: &Cx<'_, T>) -> Ops<OrbMenuAction> {
        let mut ops = Vec::new();
        let slider = [Slider::Size, Slider::Opacity]
            .into_iter()
            .find(|slider| Self::slider_rect(*slider).contains(point));
        if let Some(slider) = slider {
            // 向上滚（lines > 0）增大；触控板的小数行累计到整行前不动。
            let delta = lines.round() as i32;
            if delta != 0 {
                self.step(slider, delta, &mut ops);
            }
        }
        ops
    }

    fn window_event(&mut self, event: WindowEvent, cx: &Cx<'_, T>) -> Ops<OrbMenuAction> {
        match event {
            WindowEvent::DpiChanged { dpi, .. } if dpi != 0 && dpi != self.dpi => {
                self.dpi = dpi;
                self.measure_error(cx.text);
                vec![WindowOp::SetBounds(self.bounds()), WindowOp::Redraw]
            }
            WindowEvent::Focus(focused) => {
                vec![WindowOp::Action(OrbMenuAction::Focused(focused))]
            }
            _ => Vec::new(),
        }
    }
    fn set_look(&mut self, look: &Look) -> Ops<OrbMenuAction> {
        self.look = *look;
        vec![WindowOp::Redraw]
    }
    fn on_frame(&mut self, _: f32, cx: &Cx<'_, T>) -> Ops<OrbMenuAction> {
        let (_, finished) = self.transition.tick(cx.now);
        let mut ops = Vec::new();
        if finished && self.transition.is_fully_hidden() {
            self.visible = false;
            ops.push(WindowOp::Hide);
        }
        ops.push(WindowOp::Redraw);
        ops
    }
    fn wants_frames(&self, _: Instant) -> bool {
        self.transition.is_animating()
    }
    fn prepare_frame(&mut self, cx: &Cx<'_, T>) -> Option<FrameSpec> {
        if cx.dpi != 0 {
            self.dpi = cx.dpi;
        }
        self.visible.then(|| FrameSpec {
            bounds: self.bounds(),
            dpi: self.dpi,
            alpha: self.transition.visual(),
        })
    }
    fn draw<C: Canvas<Text = T>>(&self, canvas: &mut C, _: Instant) {
        let height = self.height();
        canvas.fill_rounded_rect(Rect::new(0.0, 0.0, WIDTH, height), RADIUS, self.color(palette::PANEL, 1.0));
        canvas.stroke_rounded_rect(
            Rect::new(0.5, 0.5, WIDTH - 0.5, height - 0.5),
            RADIUS - 0.5,
            self.color(palette::WHITE, 0.18),
            Stroke::plain(1.0),
        );
        let rows = rows();
        paint::label(
            canvas,
            "悬浮球设置",
            &text_style(14.0, 600, false),
            Rect::new(PAD, PAD, rows.close.left, PAD + HEADER),
            self.color(palette::FG, 1.0),
        );
        self.backdrop(canvas, ID_CLOSE, rows.close);
        let close_color = if self.hover == Some(ID_CLOSE) { palette::FG } else { palette::SUBTLE };
        let c = rows.close;
        Self::draw_icon(
            canvas,
            &CLOSE_ICON,
            Point::new((c.left + c.right) / 2.0, (c.top + c.bottom) / 2.0),
            self.color(close_color, 1.0),
        );
        let size = format!("{:.1}%", f32::from(self.view.size_percent) / 10.0);
        self.draw_slider(canvas, Slider::Size, rows.size_label, "大小", &size);
        let opacity = format!("{}%", self.view.opacity);
        self.draw_slider(canvas, Slider::Opacity, rows.opacity_label, "不透明度", &opacity);
        self.draw_toggle(canvas, rows.toggle);
        canvas.line(
            Point::new(PAD, rows.divider + 0.5),
            Point::new(WIDTH - PAD, rows.divider + 0.5),
            self.color(palette::WHITE, 0.11),
            Stroke::plain(1.0),
        );
        self.draw_action(canvas, ID_OPEN_MAIN, rows.actions[0], &OPEN_MAIN_ICON, "打开软件主界面", false);
        self.draw_action(canvas, ID_RESET, rows.actions[1], &RESET_ICON, "恢复大小与不透明度", false);
        self.draw_action(canvas, ID_DISABLE, rows.actions[2], &POWER_ICON, "关闭悬浮球", true);
        if let Some(error) = &self.view.error {
            let rect = Rect::new(rows.error.left, rows.error.top, rows.error.right, rows.error.top + self.error_height);
            let engine = canvas.text();
            if let Some(layout) = engine.layout(error, &text_style(12.0, 400, true), rect.width(), f32::MAX) {
                canvas.draw_text(&layout, Point::new(rect.left, rect.top), self.color(palette::ERR, 1.0), None);
            }
        }
        if let Some(id) = self.focus.filter(|_| self.keyboard) {
            let r = Self::focus_rect(id);
            canvas.stroke_rounded_rect(
                Rect::new(r.left - 2.0, r.top - 2.0, r.right + 2.0, r.bottom + 2.0),
                8.0,
                self.color(palette::ACCENT_TEXT, 1.0),
                Stroke::plain(2.0),
            );
        }
    }
    fn hit(&self) -> HitMap {
        let mut map = HitMap::new(self.dpi);
        if !self.visible {
            return map;
        }
        let rows = rows();
        map.push(ID_CLOSE, rows.close, HitKind::Button);
        map.push(ID_SIZE, rows.size, HitKind::Slider);
        map.push(ID_OPACITY, rows.opacity, HitKind::Slider);
        map.push(ID_AUTO_ENTER, rows.toggle, HitKind::Button);
        for (id, rect) in [ID_OPEN_MAIN, ID_RESET, ID_DISABLE].into_iter().zip(rows.actions) {
            map.push(id, rect, HitKind::Button);
        }
        map.set_hover(self.hover);
        map
    }
    fn glass(&self) -> Vec<Shape> {
        if self.look.profile != Profile::Css || self.look.glass.is_none() || !self.visible {
            return Vec::new();
        }
        vec![Shape::RoundedRect {
            rect: Rect::new(0.0, 0.0, WIDTH, self.height()),
            radius: RADIUS,
        }]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::FakeText;

    const WORK: PxRect = PxRect::new(0, 0, 1920, 1040);

    fn cx(text: &FakeText) -> Cx<'_, FakeText> {
        Cx {
            now: Instant::now(),
            dpi: 96,
            area: PxRect::default(),
            text,
        }
    }
    fn view() -> OrbMenuView {
        OrbMenuView {
            size_percent: 45,
            size_range: (25, 80),
            opacity: 90,
            opacity_range: (40, 100),
            auto_enter: false,
            error: None,
        }
    }
    fn shown(orb: PxRect) -> (OrbMenu, Ops<OrbMenuAction>) {
        let mut model = OrbMenu::new(96, &Look::WINDOWS_LEGACY);
        let ops = OverlayModel::<FakeText>::apply(
            &mut model,
            Command::Show {
                view: view(),
                orb,
                work: WORK,
                focus: true,
            },
            &cx(&FakeText),
        );
        (model, ops)
    }
    fn actions(ops: Ops<OrbMenuAction>) -> Vec<OrbMenuAction> {
        ops.into_iter()
            .filter_map(|op| match op {
                WindowOp::Action(a) => Some(a),
                _ => None,
            })
            .collect()
    }
    fn input(model: &mut OrbMenu, input: UiInput) -> Vec<OrbMenuAction> {
        actions(OverlayModel::<FakeText>::input(model, input, &cx(&FakeText)))
    }
    fn key(model: &mut OrbMenu, key: Key, shift: bool) -> Vec<OrbMenuAction> {
        let input = KeyInput::Down {
            key,
            modifiers: Modifiers {
                command: false,
                shift,
                alt: false,
            },
        };
        actions(OverlayModel::<FakeText>::key(model, input, &cx(&FakeText)))
    }

    #[test]
    fn panel_sits_beside_the_orb_and_flips_left_at_the_right_edge() {
        let size = (280, 340);
        // 右侧放得下：贴在球右边，与球纵向居中。
        assert_eq!(place(PxRect::new(100, 400, 56, 56), size, 8, WORK), (164, 258));
        // 右侧放不下：改到左边。
        assert_eq!(place(PxRect::new(1850, 400, 56, 56), size, 8, WORK), (1562, 258));
        // 球贴着顶边：整体夹进工作区。
        assert_eq!(place(PxRect::new(100, 0, 56, 56), size, 8, WORK), (164, 0));

        let (model, ops) = shown(PxRect::new(100, 400, 56, 56));
        let bounds = ops.iter().find_map(|op| match op {
            WindowOp::SetBounds(b) => Some(*b),
            _ => None,
        });
        assert_eq!(bounds.map(|b| (b.x, b.width)), Some((164, 280)));
        assert!(ops.contains(&WindowOp::Focus));
        assert_eq!(OverlayModel::<FakeText>::hit(&model).at((300, 300)), None);
    }

    #[test]
    fn dragging_a_slider_sends_each_new_value_and_keeps_it_against_stale_snapshots() {
        let (mut model, _) = shown(PxRect::new(100, 400, 56, 56));
        let rect = OrbMenu::slider_rect(Slider::Size);
        let y = (rect.top + rect.bottom) / 2.0;
        let (left, right) = OrbMenu::track(Slider::Size);
        assert_eq!(
            input(&mut model, UiInput::PointPress { id: ID_SIZE, point: Point::new(left - 30.0, y) }),
            vec![OrbMenuAction::SetSize(25)]
        );
        assert_eq!(
            input(&mut model, UiInput::PointDrag { id: ID_SIZE, point: Point::new(right + 30.0, y) }),
            vec![OrbMenuAction::SetSize(80)]
        );
        // 拖到同一个值不再重复发送。
        assert!(input(&mut model, UiInput::PointDrag { id: ID_SIZE, point: Point::new(right + 40.0, y) }).is_empty());
        // 拖动中到达的旧快照不把滑块拉回去，其它项照常更新。
        let stale = OrbMenuView {
            size_percent: 45,
            opacity: 70,
            ..view()
        };
        OverlayModel::<FakeText>::apply(&mut model, Command::Update(stale.clone()), &cx(&FakeText));
        assert_eq!((model.view.size_percent, model.view.opacity), (80, 70));
        input(&mut model, UiInput::PointRelease { id: ID_SIZE, point: Point::new(right, y) });
        OverlayModel::<FakeText>::apply(&mut model, Command::Update(stale), &cx(&FakeText));
        assert_eq!(model.view.size_percent, 45);
    }

    #[test]
    fn keyboard_moves_focus_adjusts_sliders_and_activates_buttons() {
        let (mut model, _) = shown(PxRect::new(100, 400, 56, 56));
        key(&mut model, Key::Tab, false);
        assert_eq!(key(&mut model, Key::Tab, false), Vec::new());
        assert_eq!(model.focus, Some(ID_SIZE));
        assert_eq!(key(&mut model, Key::Right, false), vec![OrbMenuAction::SetSize(46)]);
        assert_eq!(key(&mut model, Key::Home, false), vec![OrbMenuAction::SetSize(25)]);
        key(&mut model, Key::Tab, false);
        assert_eq!(key(&mut model, Key::PageUp, false), vec![OrbMenuAction::SetOpacity(100)]);
        key(&mut model, Key::Tab, false);
        let space = OverlayModel::<FakeText>::key(&mut model, KeyInput::Text(" ".into()), &cx(&FakeText));
        assert_eq!(actions(space), vec![OrbMenuAction::SetAutoEnter(true)]);
        // Shift+Tab 从第一项绕到最后一项。
        model.focus = Some(ID_CLOSE);
        key(&mut model, Key::Tab, true);
        assert_eq!(key(&mut model, Key::Enter, false), vec![OrbMenuAction::DisableOrb]);
        assert_eq!(key(&mut model, Key::Escape, false), vec![OrbMenuAction::Close]);
    }

    #[test]
    fn an_error_grows_the_panel_and_losing_focus_is_reported() {
        let (mut model, _) = shown(PxRect::new(100, 400, 56, 56));
        let before = model.bounds();
        let failed = OrbMenuView {
            error: Some("请先结束当前悬浮球语音输入".into()),
            ..view()
        };
        let ops = OverlayModel::<FakeText>::apply(&mut model, Command::Update(failed), &cx(&FakeText));
        let after = model.bounds();
        assert!(after.height > before.height);
        assert!(ops.contains(&WindowOp::SetBounds(after)));
        let ops = OverlayModel::<FakeText>::window_event(&mut model, WindowEvent::Focus(false), &cx(&FakeText));
        assert_eq!(actions(ops), vec![OrbMenuAction::Focused(false)]);
    }
}
