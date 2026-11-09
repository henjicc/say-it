//! 听写胶囊的状态、动画与绘制；窗口副作用由宿主按序执行。
//!
//! 布局窗口固定大小，胶囊与文字框的尺寸变化都在窗口内用弹簧动画绘制，不改窗口几何。
use crate::anim::{ease_out_cubic, Transition};
use crate::canvas::{
    Canvas, FontFamily, ParagraphAlign, TextAlign, TextEngine, TextStyle, TextTint,
};
use crate::geom::{Color, Point, PxRect, Rect, Shape, Stroke};
use crate::look::{Look, Profile};
use crate::window::{
    CursorShape, Cx, FrameSpec, Ops, OverlayModel, PointerEvent, PointerModel, PointerStep,
    WindowEvent, WindowOp,
};
use std::collections::VecDeque;
use std::time::{Duration, Instant};

// 与 indicator.rs 的 DEFAULT_INDICATOR_WIDTH/HEIGHT 保持一致。
const LOGICAL_WIDTH: f32 = 460.0;
const LOGICAL_HEIGHT: f32 = 188.0;
// 距主显示器工作区底边的逻辑像素。窗口底部自带 24px 透明内边距，
// -12 让可见内容最终停在任务栏上方约 12px（与 macOS 相对 Dock 的取值同理）。
const OFFSET_Y: f32 = -12.0;
const WRAP_PADDING_BOTTOM: f32 = 24.0;
const STACK_GAP: f32 = 10.0;

const PILL_H: f32 = 34.0;
const PILL_RADIUS: f32 = PILL_H / 2.0;
/// 录音时放得下滚动波形；处理中收窄；可跳过时悬停展开，放下「直接输入」。
const PILL_W_RECORDING: f32 = 96.0;
const PILL_W_PROCESSING: f32 = 84.0;
const PILL_W_HOVER: f32 = 116.0;

const TEXT_MAX_W: f32 = 396.0;
const TEXT_MIN_W: f32 = 96.0;
const TEXT_PAD_X: f32 = 12.0;
const TEXT_PAD_Y: f32 = 8.0;
const TEXT_MAX_LINES: u32 = 3;
const TEXT_RADIUS: f32 = 12.0;
const TEXT_SIZE: f32 = 14.0;
/// 文字框出现时从下方这么远滑入。
const TEXT_ENTER_SLIDE: f32 = 6.0;

const FALLBACK_W: f32 = 396.0;
const FALLBACK_H: f32 = 56.0;
const FALLBACK_PAD: f32 = 14.0;
const FALLBACK_ICON: f32 = 18.0;
const FALLBACK_ICON_GAP: f32 = 10.0;

/// 滚动波形：每根柱是一小段时间的音量，新柱从右侧进入、整体连续左移。
const WAVE_BARS: usize = 14;
const WAVE_BAR_W: f32 = 2.0;
const WAVE_BAR_GAP: f32 = 4.0;
const WAVE_MAX_H: f32 = 22.0;
const WAVE_MIN_H: f32 = 2.0;
/// 最左侧这么多根逐渐变淡，表示更早的声音。
const WAVE_FADE_BARS: f32 = 5.0;
/// 固定每 160ms 出一根柱，期间收到的音频块取平均：滚动慢而稳，不随数据节奏抖动。
const WAVE_BAR_S: f32 = 0.16;
/// 柱高映射：分段 RMS 到这个值才满幅。自动增益后正常说话约 0.1，落在半高附近，留出起伏余量。
const WAVE_FULL_RMS: f32 = 0.25;
/// 新柱与上一根的混合比例：相邻柱不会忽高忽低，密集说话时也不刺眼。
const WAVE_NEIGHBOR_SMOOTH: f32 = 0.35;
/// 低于这个柱高视为静音；整排都静音时波形停住，开口再继续滚动，不一直平移。
const WAVE_SILENT: f32 = 0.03;

/// 处理中：点阵铺满胶囊，用内缩的胶囊形状遮住，一道亮带左右扫过。
const DOT_STEP: f32 = 3.6;
const DOT_MASK_INSET: f32 = 3.5;
const DOT_EDGE_FADE: f32 = 2.2;
const DOT_BASE_R: f32 = 0.65;
const DOT_PEAK_R: f32 = 1.5;
/// 亮带往返的角速度（弧度/秒）。
const SCAN_SPEED: f32 = 2.1;
/// 亮带是宽而柔的高斯（σ 占扫描范围的比例），身后拖一条更长更淡的尾巴。
const SCAN_BAND: f32 = 0.13;
const SCAN_TRAIL: f32 = 4.0;
const SCAN_TRAIL_STRENGTH: f32 = 0.35;

const NOTICE_AUTO_HIDE_MS: u32 = 3500;

/// 出场时从最终位置下方这么远（逻辑像素）淡入滑入；退场反向。
const TRANSITION_SLIDE_PX: f32 = 10.0;

/// 文本新增内容的淡入时长。
const TEXT_FRESH_FADE_S: f32 = 0.18;
/// 只给 ≤10 字符的短追加淡入；长追加直接显示。
const TEXT_FRESH_FADE_MAX_CHARS: usize = 10;

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    Prepare,
    SetState(IndicatorState),
    SetText {
        text: String,
        fade: bool,
    },
    SetWaveform {
        level: f32,
        peaks: Vec<f32>,
    },
    /// 原生错误/结果通知：自定义文案，自动消失。
    ShowNotice(String),
    Hide,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IndicatorState {
    Recording,
    Processing,
    SmartProcessing,
    Fallback,
    /// 错误/结果通知（原生错误面板）：自定义短文案，几秒后自动消失。
    Notice,
}

impl IndicatorState {
    pub fn from_str(state: &str) -> Option<Self> {
        match state {
            "recording" => Some(Self::Recording),
            "processing" => Some(Self::Processing),
            "smartProcessing" => Some(Self::SmartProcessing),
            "fallback" => Some(Self::Fallback),
            _ => None,
            // Notice 不走 set_indicator_state 字符串通道，由 ShowNotice 命令进入。
        }
    }

    fn is_pill(self) -> bool {
        matches!(
            self,
            Self::Recording | Self::Processing | Self::SmartProcessing
        )
    }
}

/// 胶囊发给应用的动作。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IndicatorAction {
    /// 智能处理中点击胶囊：跳过大模型，直接输出识别原文。
    SkipSmartProcessing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IndicatorInput {
    Hover(bool),
    /// 已点击跳过：立刻退出可点击状态，给出反馈。
    Skip,
}

/// 指针层的命中快照：只有智能处理中才有可点击区域（布局坐标，DIP）。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct IndicatorHit {
    skip: Option<Rect>,
    dpi: u32,
}

/// 胶囊平时点击穿透；智能处理中整窗可交互，但只有胶囊本身响应悬停与点击。
#[derive(Debug, Default)]
pub struct IndicatorPointer {
    hovering: bool,
    pressed: bool,
}

impl IndicatorPointer {
    fn inside(hit: &IndicatorHit, client: (i32, i32)) -> bool {
        let scale = if hit.dpi == 0 { 1.0 } else { hit.dpi as f32 / 96.0 };
        let point = Point::new(client.0 as f32 / scale, client.1 as f32 / scale);
        hit.skip.is_some_and(|rect| rect.contains(point))
    }
    fn set_hover(
        &mut self,
        hovering: bool,
        steps: &mut Vec<PointerStep<IndicatorAction, IndicatorInput>>,
    ) {
        if hovering == self.hovering {
            return;
        }
        self.hovering = hovering;
        if hovering {
            steps.push(PointerStep::Op(WindowOp::TrackMouseLeave));
        }
        steps.push(PointerStep::Model(IndicatorInput::Hover(hovering)));
    }
}

impl PointerModel for IndicatorPointer {
    type Hit = IndicatorHit;
    type Input = IndicatorInput;
    type Action = IndicatorAction;
    fn event(
        &mut self,
        event: PointerEvent,
        hit: &IndicatorHit,
    ) -> Vec<PointerStep<IndicatorAction, IndicatorInput>> {
        let mut steps = Vec::new();
        match event {
            PointerEvent::Move { client, .. } => {
                self.set_hover(Self::inside(hit, client), &mut steps);
            }
            PointerEvent::LeftDown { client, .. } => {
                self.pressed = Self::inside(hit, client);
            }
            PointerEvent::LeftUp { client } => {
                if std::mem::take(&mut self.pressed) && Self::inside(hit, client) {
                    self.hovering = false;
                    steps.push(PointerStep::Model(IndicatorInput::Skip));
                    steps.push(PointerStep::Op(WindowOp::Action(
                        IndicatorAction::SkipSmartProcessing,
                    )));
                }
            }
            PointerEvent::RightUp { .. } => {}
            PointerEvent::Leave | PointerEvent::CaptureLost => {
                self.pressed = false;
                self.set_hover(false, &mut steps);
            }
        }
        steps
    }
    fn cursor(&self, hit: &IndicatorHit) -> CursorShape {
        if self.hovering && hit.skip.is_some() {
            CursorShape::Hand
        } else {
            CursorShape::Arrow
        }
    }
}

/// 临界阻尼附近的弹簧；子步长积分，帧间隔抖动时也稳定。
#[derive(Clone, Copy, Debug, PartialEq)]
struct Spring {
    x: f32,
    v: f32,
}

impl Spring {
    const fn at(x: f32) -> Self {
        Self { x, v: 0.0 }
    }
    fn snap(&mut self, x: f32) {
        *self = Self::at(x);
    }
    fn step(&mut self, target: f32, dt: f32, stiffness: f32, damping: f32) {
        let mut left = dt.clamp(0.0, 0.1);
        while left > 0.0 {
            let h = left.min(1.0 / 240.0);
            let accel = stiffness * (target - self.x) - damping * self.v;
            self.v += accel * h;
            self.x += self.v * h;
            left -= h;
        }
    }
    fn settled(&self, target: f32, epsilon: f32) -> bool {
        (self.x - target).abs() < epsilon && self.v.abs() < epsilon
    }
}

/// 文字框的目标几何：宽度贴合最长一行，高度最多三行，超出时向上滚动。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct TextTarget {
    width: f32,
    height: f32,
    scroll: f32,
}

pub struct Indicator {
    state: Option<IndicatorState>,
    text: String,
    /// 文本中「新增内容」的起始字符索引，配合 fresh_started 做淡入。
    fresh_from: usize,
    fresh_started: Option<Instant>,
    /// 屏幕上的波形柱（最旧在前），比可见数量多一根用于滚入。
    wave_bars: VecDeque<f32>,
    /// 本根柱周期内收到的音频块柱高之和与块数。
    wave_acc_sum: f32,
    wave_acc_count: u32,
    /// 0..1：当前柱向左滚过的比例；停住时保持 1.0。
    wave_scroll: f32,
    /// 处理中扫描动画的时间轴。
    scan_t: f32,
    pill_w: Spring,
    /// 0 = 录音波形，1 = 处理中点阵。
    morph: Spring,
    hover: Spring,
    hovering: bool,
    /// 宿主能把指针事件交给胶囊时才允许点击跳过（Windows）；否则始终点击穿透。
    clickable: bool,
    interactive: bool,
    text_target: TextTarget,
    text_w: Spring,
    text_h: Spring,
    text_scroll: Spring,
    text_alpha: Spring,
    /// Notice 态的自定义文案。
    notice_text: String,
    dpi: u32,
    transition: Transition,
    notice_deadline: Option<Instant>,
    look: Look,
}

impl Indicator {
    pub fn new(look: &Look) -> Self {
        Self {
            state: None,
            text: String::new(),
            fresh_from: 0,
            fresh_started: None,
            wave_bars: VecDeque::from(vec![0.0; WAVE_BARS + 1]),
            wave_acc_sum: 0.0,
            wave_acc_count: 0,
            wave_scroll: 0.0,
            scan_t: 0.0,
            pill_w: Spring::at(PILL_W_RECORDING),
            morph: Spring::at(0.0),
            hover: Spring::at(0.0),
            hovering: false,
            clickable: false,
            interactive: false,
            text_target: TextTarget::default(),
            text_w: Spring::at(0.0),
            text_h: Spring::at(0.0),
            text_scroll: Spring::at(0.0),
            text_alpha: Spring::at(0.0),
            notice_text: String::new(),
            dpi: 96,
            transition: Transition::new(false),
            notice_deadline: None,
            look: *look,
        }
    }
    /// 宿主接通了指针事件：智能处理中允许点击胶囊跳过。
    pub fn clickable(mut self) -> Self {
        self.clickable = true;
        self
    }
    fn reset_wave(&mut self) {
        self.wave_bars = VecDeque::from(vec![0.0; WAVE_BARS + 1]);
        self.wave_acc_sum = 0.0;
        self.wave_acc_count = 0;
        self.wave_scroll = 0.0;
    }
    fn clear_content(&mut self) {
        self.state = None;
        self.text.clear();
        self.text_target = TextTarget::default();
        self.text_alpha.snap(0.0);
        self.reset_wave();
        self.hovering = false;
        self.hover.snap(0.0);
    }
    /// 只有智能处理中可以点击跳过；其余状态保持点击穿透。
    fn sync_interactive(&mut self, ops: &mut Ops<IndicatorAction>) {
        let want = self.clickable && self.state == Some(IndicatorState::SmartProcessing);
        if !want {
            self.hovering = false;
        }
        if want != self.interactive {
            self.interactive = want;
            ops.push(WindowOp::SetInteractive(want));
        }
    }
    fn begin_exit(&mut self) -> Ops<IndicatorAction> {
        self.notice_deadline = None;
        let mut ops = Vec::new();
        if !self.transition.hide() {
            self.clear_content();
            ops.push(WindowOp::Hide);
        }
        self.hovering = false;
        if self.interactive {
            self.interactive = false;
            ops.push(WindowOp::SetInteractive(false));
        }
        ops
    }
    fn pill_target(&self) -> f32 {
        match self.state {
            Some(IndicatorState::Recording) | None => PILL_W_RECORDING,
            _ if self.hovering => PILL_W_HOVER,
            _ => PILL_W_PROCESSING,
        }
    }
    fn morph_target(&self) -> f32 {
        match self.state {
            Some(IndicatorState::Processing | IndicatorState::SmartProcessing) => 1.0,
            _ => 0.0,
        }
    }
    /// 实时文字只在录音时显示；处理中收起，只留胶囊。
    fn text_visible(&self) -> bool {
        !self.text.is_empty() && self.state == Some(IndicatorState::Recording)
    }
    /// 隐藏后重新出现时不从旧尺寸动画过来，直接落在目标上。
    fn snap_geometry(&mut self) {
        self.pill_w.snap(self.pill_target());
        self.morph.snap(self.morph_target());
        self.hover.snap(0.0);
    }
    fn measure_text<T: TextEngine>(&mut self, text: &T) {
        if self.text.is_empty() {
            return;
        }
        let inner = TEXT_MAX_W - TEXT_PAD_X * 2.0;
        let Some(layout) = text.layout(&self.text, &text_style(TEXT_SIZE, false), inner, 10_000.0)
        else {
            return;
        };
        let metrics = text.metrics(&layout);
        let lines = metrics.line_count.max(1);
        let line_h = metrics.height / lines as f32;
        let visible = lines.min(TEXT_MAX_LINES);
        self.text_target = TextTarget {
            width: (metrics.width + TEXT_PAD_X * 2.0).clamp(TEXT_MIN_W, TEXT_MAX_W),
            height: visible as f32 * line_h + TEXT_PAD_Y * 2.0,
            scroll: (lines - visible) as f32 * line_h,
        };
        if self.text_alpha.x < 0.05 {
            self.text_w.snap(self.text_target.width);
            self.text_h.snap(self.text_target.height);
            self.text_scroll.snap(self.text_target.scroll);
        }
    }
    /// 记入本根柱周期：分段 RMS 取平均后按留有余量的曲线映射。
    fn push_wave_chunk(&mut self, level: f32, peaks: &[f32]) {
        let rms = if peaks.is_empty() {
            level
        } else {
            peaks.iter().sum::<f32>() / peaks.len() as f32
        };
        self.wave_acc_sum += bar_level(rms);
        self.wave_acc_count += 1;
    }
    /// 到出柱时刻：周期内的平均与上一根混合后滚入；没收到数据或整排静音时停在当前位置。
    /// 停住时 scroll 保持 1.0，与刚滚入一根时的位置完全一致，恢复时不会跳。
    fn advance_wave(&mut self, dt: f32) {
        self.wave_scroll += dt / WAVE_BAR_S;
        if self.wave_scroll < 1.0 {
            return;
        }
        if self.wave_acc_count == 0 {
            self.wave_scroll = 1.0;
            return;
        }
        let target = self.wave_acc_sum / self.wave_acc_count as f32;
        self.wave_acc_sum = 0.0;
        self.wave_acc_count = 0;
        let previous = self.wave_bars.back().copied().unwrap_or(0.0);
        let next = previous * WAVE_NEIGHBOR_SMOOTH + target * (1.0 - WAVE_NEIGHBOR_SMOOTH);
        if next < WAVE_SILENT && self.wave_bars.iter().all(|bar| *bar < WAVE_SILENT) {
            self.wave_scroll = 1.0;
            return;
        }
        self.wave_bars.pop_front();
        self.wave_bars.push_back(next);
        self.wave_scroll = (self.wave_scroll - 1.0).min(0.999);
    }
    /// 每帧推进全部动画。
    fn tick_animations(&mut self, dt: f32, now: Instant) {
        if self.state == Some(IndicatorState::Recording) {
            self.advance_wave(dt);
        }
        if self.morph.x > 0.001 || self.morph_target() > 0.0 {
            self.scan_t += dt;
        }
        self.pill_w.step(self.pill_target(), dt, 260.0, 28.0);
        self.morph.step(self.morph_target(), dt, 150.0, 22.0);
        let hover = if self.hovering { 1.0 } else { 0.0 };
        self.hover.step(hover, dt, 320.0, 30.0);
        let alpha = if self.text_visible() { 1.0 } else { 0.0 };
        self.text_alpha.step(alpha, dt, 220.0, 28.0);
        self.text_w.step(self.text_target.width, dt, 240.0, 30.0);
        self.text_h.step(self.text_target.height, dt, 240.0, 30.0);
        self.text_scroll.step(self.text_target.scroll, dt, 200.0, 28.0);
        if self.fresh_started.is_some_and(|start| {
            now.saturating_duration_since(start).as_secs_f32() >= TEXT_FRESH_FADE_S
        }) {
            self.fresh_started = None;
        }
    }
    fn springs_settled(&self) -> bool {
        let text_alpha = if self.text_visible() { 1.0 } else { 0.0 };
        let hover = if self.hovering { 1.0 } else { 0.0 };
        self.pill_w.settled(self.pill_target(), 0.05)
            && self.morph.settled(self.morph_target(), 0.002)
            && self.hover.settled(hover, 0.002)
            && self.text_alpha.settled(text_alpha, 0.002)
            && self.text_w.settled(self.text_target.width, 0.05)
            && self.text_h.settled(self.text_target.height, 0.05)
            && self.text_scroll.settled(self.text_target.scroll, 0.05)
    }

    /// 文本新增部分的淡入进度（1.0 = 已完成/无新增）。
    fn fresh_alpha(&self, now: Instant) -> f32 {
        self.fresh_started
            .map(|start| {
                let elapsed = now.saturating_duration_since(start).as_secs_f32();
                let t = (elapsed / TEXT_FRESH_FADE_S).min(1.0);
                ease_out_cubic(t)
            })
            .unwrap_or(1.0)
    }
    fn content_bottom() -> f32 {
        LOGICAL_HEIGHT - WRAP_PADDING_BOTTOM
    }
    fn pill_rect(&self) -> Rect {
        let width = self.pill_w.x;
        let bottom = Self::content_bottom();
        Rect::new(
            (LOGICAL_WIDTH - width) / 2.0,
            bottom - PILL_H,
            (LOGICAL_WIDTH + width) / 2.0,
            bottom,
        )
    }
    fn text_rect(&self) -> Rect {
        let width = self.text_w.x;
        let bottom =
            self.pill_rect().top - STACK_GAP + (1.0 - self.text_alpha.x) * TEXT_ENTER_SLIDE;
        Rect::new(
            (LOGICAL_WIDTH - width) / 2.0,
            bottom - self.text_h.x,
            (LOGICAL_WIDTH + width) / 2.0,
            bottom,
        )
    }
}

impl<T: TextEngine> OverlayModel<T> for Indicator {
    type Command = Command;
    type Action = IndicatorAction;
    type Pointer = IndicatorPointer;

    fn apply(&mut self, command: Command, cx: &Cx<'_, T>) -> Ops<IndicatorAction> {
        match command {
            Command::Prepare => {
                self.notice_deadline = None;
                self.text.clear();
                self.text_target = TextTarget::default();
                self.fresh_from = 0;
                self.fresh_started = None;
                self.reset_wave();
                Vec::new()
            }
            Command::SetState(state) => {
                self.notice_deadline = None;
                let reappearing = self.transition.is_fully_hidden() || self.state.is_none();
                self.state = Some(state);
                if state == IndicatorState::Fallback {
                    self.text.clear();
                    self.reset_wave();
                }
                if reappearing {
                    self.snap_geometry();
                }
                self.transition.show();
                let mut ops = vec![WindowOp::Redraw, WindowOp::RaiseTopmost];
                self.sync_interactive(&mut ops);
                ops
            }
            Command::SetText { text, fade } => {
                // 只有短追加或显式换新才淡入；修订已有内容直接替换。
                let fresh_from = if fade {
                    0
                } else if text.starts_with(&self.text) {
                    self.text.chars().count()
                } else {
                    usize::MAX
                };
                let fresh_len = if fresh_from == usize::MAX {
                    usize::MAX
                } else {
                    text.chars().count().saturating_sub(fresh_from)
                };
                self.fresh_from = fresh_from;
                self.fresh_started =
                    (fresh_len > 0 && fresh_len <= TEXT_FRESH_FADE_MAX_CHARS).then_some(cx.now);
                self.text = text;
                self.measure_text(cx.text);
                if self.state.is_some() {
                    vec![WindowOp::Redraw]
                } else {
                    Vec::new()
                }
            }
            Command::SetWaveform { level, peaks } => {
                self.push_wave_chunk(level, &peaks);
                Vec::new()
            }
            Command::ShowNotice(text) => {
                self.text.clear();
                self.fresh_from = 0;
                self.fresh_started = None;
                self.reset_wave();
                self.notice_text = text;
                self.state = Some(IndicatorState::Notice);
                self.transition.show();
                self.notice_deadline =
                    Some(cx.now + Duration::from_millis(NOTICE_AUTO_HIDE_MS as u64));
                let mut ops = vec![WindowOp::Redraw, WindowOp::RaiseTopmost];
                self.sync_interactive(&mut ops);
                ops
            }
            Command::Hide => self.begin_exit(),
        }
    }
    fn input(&mut self, input: IndicatorInput, _: &Cx<'_, T>) -> Ops<IndicatorAction> {
        let mut ops = vec![WindowOp::Redraw];
        match input {
            IndicatorInput::Hover(hovering) => self.hovering = hovering && self.interactive,
            IndicatorInput::Skip => {
                if self.state == Some(IndicatorState::SmartProcessing) {
                    self.state = Some(IndicatorState::Processing);
                    self.sync_interactive(&mut ops);
                }
            }
        }
        ops
    }
    fn window_event(&mut self, event: WindowEvent, cx: &Cx<'_, T>) -> Ops<IndicatorAction> {
        if let WindowEvent::DpiChanged { dpi, .. } = event {
            if dpi != 0 {
                self.dpi = dpi;
            }
            return vec![WindowOp::Redraw];
        }
        if event == WindowEvent::AreaChanged && self.state.is_some() {
            self.dpi = cx.dpi;
            return vec![WindowOp::Redraw];
        }
        Vec::new()
    }
    fn set_look(&mut self, look: &Look) -> Ops<IndicatorAction> {
        self.look = *look;
        vec![WindowOp::Redraw]
    }
    fn on_frame(&mut self, dt: f32, cx: &Cx<'_, T>) -> Ops<IndicatorAction> {
        let mut ops = if self
            .notice_deadline
            .is_some_and(|deadline| deadline <= cx.now)
        {
            self.begin_exit()
        } else {
            Vec::new()
        };
        self.tick_animations(dt, cx.now);
        let (_, finished) = self.transition.tick(cx.now);
        if finished && self.transition.is_fully_hidden() {
            self.clear_content();
            ops.push(WindowOp::Hide);
        }
        ops.push(WindowOp::Redraw);
        ops
    }
    /// 录音波形与处理中扫描持续动画；其余状态只在弹簧、淡入、倒计时或入退场未完成时要帧。
    fn wants_frames(&self, _: Instant) -> bool {
        self.transition.is_animating()
            || self.state.is_some_and(IndicatorState::is_pill)
            || self.fresh_started.is_some()
            || self.notice_deadline.is_some()
            || (self.state.is_some() && !self.springs_settled())
    }
    fn prepare_frame(&mut self, cx: &Cx<'_, T>) -> Option<FrameSpec> {
        self.state?;
        let dpi = cx.dpi;
        self.dpi = dpi;
        let scale = if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 };
        let width = (LOGICAL_WIDTH * scale).round() as i32;
        let height = (LOGICAL_HEIGHT * scale).round() as i32;
        let x = cx.area.x + (cx.area.width - width) / 2;
        let y = cx.area.bottom() - height - (OFFSET_Y * scale).round() as i32;
        let visual = self.transition.visual();
        let slide = ((1.0 - visual) * TRANSITION_SLIDE_PX * scale).round() as i32;
        Some(FrameSpec {
            bounds: PxRect::new(x, y + slide, width, height),
            dpi,
            alpha: visual,
        })
    }
    fn draw<C: Canvas<Text = T>>(&self, canvas: &mut C, now: Instant) {
        self.draw_content(canvas, now);
    }
    fn hit(&self) -> IndicatorHit {
        let skip = (self.interactive && self.transition.visual() > 0.5).then(|| {
            let pill = self.pill_rect();
            Rect::new(pill.left - 2.0, pill.top - 2.0, pill.right + 2.0, pill.bottom + 2.0)
        });
        IndicatorHit {
            skip,
            dpi: self.dpi,
        }
    }
    fn glass(&self) -> Vec<Shape> {
        if self.look.profile != Profile::Css || self.look.glass.is_none() {
            return Vec::new();
        }
        let Some(state) = self.state else {
            return Vec::new();
        };
        if matches!(state, IndicatorState::Fallback | IndicatorState::Notice) {
            return vec![Shape::RoundedRect {
                rect: Self::notice_rect(),
                radius: TEXT_RADIUS,
            }];
        }
        let mut shapes = vec![Shape::RoundedRect {
            rect: self.pill_rect(),
            radius: PILL_RADIUS,
        }];
        if self.text_alpha.x > 0.01 {
            shapes.push(Shape::RoundedRect {
                rect: self.text_rect(),
                radius: TEXT_RADIUS,
            });
        }
        shapes
    }
}

impl Indicator {
    fn notice_rect() -> Rect {
        let bottom = Self::content_bottom();
        Rect::new(
            (LOGICAL_WIDTH - FALLBACK_W) / 2.0,
            bottom - FALLBACK_H,
            (LOGICAL_WIDTH + FALLBACK_W) / 2.0,
            bottom,
        )
    }
    /// 胶囊、文字框与通知面板共用：深色底加一圈内侧浅色细线；深色桌面靠亮线、浅色桌面靠深底看清轮廓。
    fn draw_panel<C: Canvas>(
        &self,
        canvas: &mut C,
        rect: Rect,
        radius: f32,
        alpha: f32,
        emphasis: f32,
    ) {
        canvas.fill_rounded_rect(
            rect,
            radius,
            self.look.color(23.0 / 255.0, 24.0 / 255.0, 27.0 / 255.0, 0.96 * alpha),
        );
        let inner = Rect::new(
            rect.left + 0.5,
            rect.top + 0.5,
            rect.right - 0.5,
            rect.bottom - 0.5,
        );
        canvas.stroke_rounded_rect(
            inner,
            radius - 0.5,
            self.look
                .color(1.0, 1.0, 1.0, (0.34 + 0.14 * emphasis) * alpha),
            Stroke::plain(1.0),
        );
    }
    fn draw_content<C: Canvas>(&self, canvas: &mut C, now: Instant) {
        let Some(state) = self.state else { return };
        if state == IndicatorState::Fallback {
            self.draw_notice_panel(
                canvas,
                Self::content_bottom(),
                "已经把结果放到你的剪贴板里了，你粘贴就可以用了",
                false,
            );
            return;
        }
        if state == IndicatorState::Notice {
            self.draw_notice_panel(canvas, Self::content_bottom(), &self.notice_text, true);
            return;
        }
        if self.text_alpha.x > 0.01 {
            self.draw_text_box(canvas, now);
        }
        let pill = self.pill_rect();
        self.draw_panel(canvas, pill, PILL_RADIUS, 1.0, self.hover.x);
        let content = 1.0 - self.hover.x.clamp(0.0, 1.0);
        let morph = self.morph.x.clamp(0.0, 1.0);
        if morph < 0.999 {
            self.draw_waveform(canvas, pill, (1.0 - morph) * content, 1.0 - morph);
        }
        if morph > 0.001 {
            self.draw_scan(canvas, pill, morph * content);
        }
        if self.hover.x > 0.01 {
            self.draw_skip_hint(canvas, pill, self.hover.x.clamp(0.0, 1.0));
        }
    }
    fn draw_text_box<C: Canvas>(&self, canvas: &mut C, now: Instant) {
        let alpha = self.text_alpha.x.clamp(0.0, 1.0);
        let rect = self.text_rect();
        self.draw_panel(canvas, rect, TEXT_RADIUS, alpha, 0.0);
        if self.text.is_empty() {
            return;
        }
        // 换行宽度固定为最大宽度：框宽在动画时，已排好的行不会跟着重新折行。
        let wrap_w = TEXT_MAX_W - TEXT_PAD_X * 2.0;
        let Some(layout) =
            canvas
                .text()
                .layout(&self.text, &text_style(TEXT_SIZE, false), wrap_w, 10_000.0)
        else {
            return;
        };
        let fade = self.fresh_alpha(now);
        let tint = if fade < 1.0 && self.fresh_from < self.text.chars().count() {
            let utf16_start: u32 = self
                .text
                .chars()
                .take(self.fresh_from)
                .map(|c| c.len_utf16() as u32)
                .sum();
            let total = self.text.encode_utf16().count() as u32;
            (utf16_start < total).then_some(TextTint {
                utf16_start,
                utf16_len: total - utf16_start,
                color: fg(&self.look, fade * alpha),
            })
        } else {
            None
        };
        let inner = Rect::new(
            rect.left + TEXT_PAD_X,
            rect.top + TEXT_PAD_Y,
            rect.right - TEXT_PAD_X,
            rect.bottom - TEXT_PAD_Y,
        );
        canvas.save();
        canvas.clip_rect(inner);
        canvas.draw_text(
            &layout,
            Point::new(inner.left, inner.top - self.text_scroll.x),
            fg(&self.look, alpha),
            tint,
        );
        canvas.restore();
    }
    /// 录音：柱子连续左移，最左侧几根渐隐；高度随 `shrink` 收向中线，过渡到点阵。
    fn draw_waveform<C: Canvas>(&self, canvas: &mut C, pill: Rect, alpha: f32, shrink: f32) {
        if alpha < 0.01 {
            return;
        }
        let step = WAVE_BAR_W + WAVE_BAR_GAP;
        let span = WAVE_BARS as f32 * step - WAVE_BAR_GAP;
        let center_x = (pill.left + pill.right) / 2.0;
        let center_y = (pill.top + pill.bottom) / 2.0;
        let left = center_x - span / 2.0;
        let clip = Rect::new(
            pill.left + PILL_RADIUS * 0.5,
            pill.top,
            pill.right - PILL_RADIUS * 0.5,
            pill.bottom,
        );
        canvas.save();
        canvas.clip_rect(clip);
        for (index, level) in self.wave_bars.iter().enumerate() {
            let x = left + (index as f32 - self.wave_scroll) * step;
            let fade = ((x - left + step) / (WAVE_FADE_BARS * step)).clamp(0.0, 1.0);
            let height =
                (WAVE_MIN_H + level.clamp(0.0, 1.0) * (WAVE_MAX_H - WAVE_MIN_H)) * shrink;
            let height = height.max(WAVE_BAR_W);
            canvas.fill_rounded_rect(
                Rect::new(
                    x,
                    center_y - height / 2.0,
                    x + WAVE_BAR_W,
                    center_y + height / 2.0,
                ),
                WAVE_BAR_W / 2.0,
                fg(&self.look, alpha * (0.35 + 0.65 * fade)),
            );
        }
        canvas.restore();
    }
    /// 处理中：点阵铺满胶囊、由内缩的胶囊形状遮住（点不碰描边），亮带左右往返并拖着渐淡的尾巴。
    fn draw_scan<C: Canvas>(&self, canvas: &mut C, pill: Rect, alpha: f32) {
        if alpha < 0.01 {
            return;
        }
        let width = pill.width();
        let height = pill.height();
        let center_x = (pill.left + pill.right) / 2.0;
        let center_y = (pill.top + pill.bottom) / 2.0;
        let mask_r = height / 2.0 - DOT_MASK_INSET;
        let straight = width / 2.0 - height / 2.0;
        let cols = (width / DOT_STEP).ceil() as i32 + 1;
        let rows = (height / DOT_STEP).ceil() as i32 + 1;
        let start_x = center_x - (cols - 1) as f32 * DOT_STEP / 2.0;
        let start_y = center_y - (rows - 1) as f32 * DOT_STEP / 2.0;
        let band_left = pill.left + DOT_MASK_INSET + 1.5;
        let band_right = pill.right - DOT_MASK_INSET - 1.5;
        let phase = self.scan_t * SCAN_SPEED;
        let sweep = phase.sin() * 0.5 + 0.5;
        // 尾巴强度随速度变化：折返点速度为零、尾巴恰好消失，换到另一侧时不会闪。
        let velocity = phase.cos();
        let direction = velocity.signum();
        let trail_strength = SCAN_TRAIL_STRENGTH * velocity.abs();
        for col in 0..cols {
            let x = start_x + col as f32 * DOT_STEP;
            let u = (x - band_left) / (band_right - band_left).max(1.0);
            let behind = (sweep - u) * direction;
            let distance = (u - sweep).abs();
            let band = (-(distance * distance) / (2.0 * SCAN_BAND * SCAN_BAND)).exp();
            let trail = if behind > 0.0 {
                (-behind * SCAN_TRAIL).exp() * trail_strength
            } else {
                0.0
            };
            let lit = (band + trail).min(1.0);
            for row in 0..rows {
                let y = start_y + row as f32 * DOT_STEP;
                let dx = ((x - center_x).abs() - straight).max(0.0);
                let edge_distance = mask_r - (dx * dx + (y - center_y).powi(2)).sqrt();
                if edge_distance <= 0.0 {
                    continue;
                }
                let edge = (edge_distance / DOT_EDGE_FADE).min(1.0);
                let radius = ((DOT_BASE_R + (DOT_PEAK_R - DOT_BASE_R) * lit)
                    * (0.6 + 0.4 * edge))
                    .min(edge_distance);
                let opacity = (0.2 + 0.75 * lit) * edge * alpha;
                if opacity < 0.01 || radius < 0.15 {
                    continue;
                }
                canvas.fill_circle(Point::new(x, y), radius, fg(&self.look, opacity));
            }
        }
    }
    /// 悬停提示：回车图标 +「直接输入」。
    fn draw_skip_hint<C: Canvas>(&self, canvas: &mut C, pill: Rect, alpha: f32) {
        const ICON_W: f32 = 9.0;
        const ICON_GAP: f32 = 6.0;
        let color = fg(&self.look, alpha);
        let label = canvas
            .text()
            .layout("直接输入", &text_style(12.0, false), 200.0, 40.0);
        let metrics = label.as_ref().map(|layout| canvas.text().metrics(layout));
        let text_w = metrics.as_ref().map_or(0.0, |metrics| metrics.width);
        let total = ICON_W + ICON_GAP + text_w;
        let left = (pill.left + pill.right - total) / 2.0;
        let cy = (pill.top + pill.bottom) / 2.0;
        let stroke = Stroke::plain(1.4);
        let right = left + ICON_W;
        canvas.line(Point::new(right, cy - 4.0), Point::new(right, cy + 1.5), color, stroke);
        canvas.line(Point::new(right, cy + 1.5), Point::new(left, cy + 1.5), color, stroke);
        canvas.line(Point::new(left, cy + 1.5), Point::new(left + 3.0, cy - 1.5), color, stroke);
        canvas.line(Point::new(left, cy + 1.5), Point::new(left + 3.0, cy + 4.5), color, stroke);
        if let (Some(layout), Some(metrics)) = (label, metrics) {
            canvas.draw_text(
                &layout,
                Point::new(left + ICON_W + ICON_GAP, cy - metrics.height / 2.0),
                color,
                None,
            );
        }
    }
    fn draw_notice_panel<C: Canvas>(
        &self,
        canvas: &mut C,
        content_bottom: f32,
        text: &str,
        warn: bool,
    ) {
        let rect = Rect::new(
            (LOGICAL_WIDTH - FALLBACK_W) / 2.0,
            content_bottom - FALLBACK_H,
            (LOGICAL_WIDTH + FALLBACK_W) / 2.0,
            content_bottom,
        );
        self.draw_panel(canvas, rect, TEXT_RADIUS, 1.0, 0.0);
        let color = if warn {
            self.look.color(229.0 / 255.0, 114.0 / 255.0, 108.0 / 255.0, 1.0)
        } else {
            self.look.color(163.0 / 255.0, 166.0 / 255.0, 173.0 / 255.0, 1.0)
        };
        let icon_cx = rect.left + FALLBACK_PAD + FALLBACK_ICON / 2.0;
        let icon_cy = (rect.top + rect.bottom) / 2.0;
        if warn {
            let r = FALLBACK_ICON / 2.0 - 1.0;
            let top = Point::new(icon_cx, icon_cy - r);
            let left = Point::new(icon_cx - r, icon_cy + r * 0.8);
            let right = Point::new(icon_cx + r, icon_cy + r * 0.8);
            canvas.line(top, left, color, Stroke::plain(1.8));
            canvas.line(left, right, color, Stroke::plain(1.8));
            canvas.line(right, top, color, Stroke::plain(1.8));
            canvas.line(
                Point::new(icon_cx, icon_cy - r * 0.35),
                Point::new(icon_cx, icon_cy + r * 0.3),
                color,
                Stroke::plain(1.8),
            );
            canvas.fill_circle(Point::new(icon_cx, icon_cy + r * 0.55), 1.2, color);
        } else {
            canvas.stroke_circle(
                Point::new(icon_cx, icon_cy),
                FALLBACK_ICON / 2.0 - 1.0,
                color,
                Stroke::plain(1.8),
            );
            let check = [
                (icon_cx - 3.5, icon_cy + 0.5),
                (icon_cx - 0.8, icon_cy + 3.2),
                (icon_cx + 4.0, icon_cy - 3.0),
            ];
            canvas.line(
                Point::new(check[0].0, check[0].1),
                Point::new(check[1].0, check[1].1),
                color,
                Stroke::plain(2.0),
            );
            canvas.line(
                Point::new(check[1].0, check[1].1),
                Point::new(check[2].0, check[2].1),
                color,
                Stroke::plain(2.0),
            );
        }
        let bounds = Rect::new(
            rect.left + FALLBACK_PAD + FALLBACK_ICON + FALLBACK_ICON_GAP,
            rect.top,
            rect.right - FALLBACK_PAD,
            rect.bottom,
        );
        if let Some(layout) = canvas.text().layout(
            text,
            &text_style(13.0, true),
            bounds.width(),
            bounds.height(),
        ) {
            canvas.draw_text(
                &layout,
                Point::new(bounds.left, bounds.top),
                fg(&self.look, 1.0),
                None,
            );
        }
    }
}

/// 胶囊柱高：分段 RMS 线性归一后略微压缩，弱音也看得到，正常说话不会顶满。
fn bar_level(rms: f32) -> f32 {
    let normalized = if rms.is_nan() { 0.0 } else { (rms / WAVE_FULL_RMS).clamp(0.0, 1.0) };
    normalized.powf(0.8)
}

fn text_style(size: f32, centered: bool) -> TextStyle<'static> {
    TextStyle {
        family: FontFamily::SystemUi,
        size,
        weight: 400,
        align: TextAlign::Leading,
        paragraph: if centered {
            ParagraphAlign::Center
        } else {
            ParagraphAlign::Near
        },
        wrap: true,
        line_spacing: None,
    }
}

/// 主前景色（与主窗口 --color-fg 一致）。
fn fg(look: &Look, alpha: f32) -> Color {
    look.color(236.0 / 255.0, 237.0 / 255.0, 239.0 / 255.0, alpha)
}

/// 感知响度曲线，移植自 ui/src/floating-orb/interaction.ts 的 floatingOrbWaveScale。
pub fn wave_scale(value: f32) -> f32 {
    // JS 侧 `Number(value) || 0` 会把 NaN 归 0。
    let normalized = if value.is_nan() {
        0.0
    } else {
        value.clamp(0.0, 1.0)
    };
    (normalized.sqrt() * 1.8).min(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{Draw, FakeText, RecordingCanvas};

    fn cx(now: Instant, text: &FakeText) -> Cx<'_, FakeText> {
        Cx {
            now,
            dpi: 144,
            area: PxRect::new(0, 0, 1920, 1040),
            text,
        }
    }
    fn settled(model: &mut Indicator, now: Instant, text: &FakeText) {
        model.on_frame(0.016, &cx(now, text));
        model.on_frame(0.320, &cx(now + Duration::from_millis(320), text));
    }
    #[test]
    fn notice_deadline_starts_exit_once_and_hides_after_transition() {
        let now = Instant::now();
        let text = FakeText;
        let mut model = Indicator::new(&Look::WINDOWS_LEGACY);
        assert_eq!(
            model.apply(Command::ShowNotice("通知".into()), &cx(now, &text)),
            vec![WindowOp::Redraw, WindowOp::RaiseTopmost]
        );
        settled(&mut model, now, &text);
        model.on_frame(0.016, &cx(now + Duration::from_millis(3499), &text));
        assert_eq!(model.transition.visual(), 1.0);
        assert_eq!(
            model.on_frame(0.001, &cx(now + Duration::from_millis(3500), &text)),
            vec![WindowOp::Redraw]
        );
        assert!(model.transition.is_animating());
        assert_eq!(model.notice_deadline, None);
        assert_eq!(
            model.on_frame(0.140, &cx(now + Duration::from_millis(3640), &text)),
            vec![WindowOp::Hide, WindowOp::Redraw]
        );
        assert!(!<Indicator as OverlayModel<FakeText>>::wants_frames(
            &model, now
        ));
        assert!(model.prepare_frame(&cx(now, &text)).is_none());
    }
    #[test]
    fn prepare_cancels_notice_deadline_but_keeps_visible_notice() {
        let now = Instant::now();
        let text = FakeText;
        let mut model = Indicator::new(&Look::WINDOWS_LEGACY);
        model.apply(Command::ShowNotice("保留通知".into()), &cx(now, &text));
        settled(&mut model, now, &text);
        model.apply(Command::Prepare, &cx(now + Duration::from_secs(1), &text));
        model.on_frame(4.0, &cx(now + Duration::from_secs(5), &text));
        assert_eq!(model.state, Some(IndicatorState::Notice));
        assert_eq!(model.notice_text, "保留通知");
        assert_eq!(model.transition.visual(), 1.0);
    }
    #[test]
    fn hidden_hide_is_immediate_and_hide_show_before_first_frame_stays_visible() {
        let now = Instant::now();
        let text = FakeText;
        let mut model = Indicator::new(&Look::WINDOWS_LEGACY);
        assert_eq!(
            model.apply(Command::Hide, &cx(now, &text)),
            vec![WindowOp::Hide]
        );
        model.apply(
            Command::SetState(IndicatorState::Recording),
            &cx(now, &text),
        );
        settled(&mut model, now, &text);
        assert!(model.apply(Command::Hide, &cx(now, &text)).is_empty());
        model.apply(
            Command::SetState(IndicatorState::Processing),
            &cx(now, &text),
        );
        model.on_frame(1.0, &cx(now + Duration::from_secs(1), &text));
        assert_eq!(model.state, Some(IndicatorState::Processing));
        assert_eq!(model.transition.visual(), 1.0);
    }
    #[test]
    fn fresh_only_fades_one_to_ten_new_characters_and_tracks_utf16() {
        let now = Instant::now();
        let text = FakeText;
        for (old, new, fade, expected) in [
            ("", "", false, None),
            ("旧", "旧", false, None),
            ("旧", "旧新", false, Some(1)),
            ("旧", "旧一二三四五六七八九十", false, Some(1)),
            ("旧", "旧一二三四五六七八九十一", false, None),
            ("旧", "修订", false, None),
            ("旧", "换新", true, Some(0)),
            ("旧", "一二三四五六七八九十一", true, None),
            ("😀", "😀新增", false, Some(1)),
        ] {
            let mut model = Indicator::new(&Look::WINDOWS_LEGACY);
            model.apply(
                Command::SetText {
                    text: old.into(),
                    fade: false,
                },
                &cx(now, &text),
            );
            model.apply(
                Command::SetState(IndicatorState::Recording),
                &cx(now, &text),
            );
            model.apply(
                Command::SetText {
                    text: new.into(),
                    fade,
                },
                &cx(now, &text),
            );
            assert_eq!(
                model.fresh_started.map(|_| model.fresh_from),
                expected,
                "{old:?} → {new:?}"
            );
            if old == "😀" {
                model.on_frame(0.08, &cx(now + Duration::from_millis(80), &text));
                let mut canvas = RecordingCanvas::new(144);
                model.draw(&mut canvas, now + Duration::from_millis(90));
                assert!(canvas.calls.iter().any(|call| matches!(
                    call,
                    Draw::Text(
                        _,
                        _,
                        _,
                        Some(TextTint {
                            utf16_start: 2,
                            utf16_len: 2,
                            ..
                        })
                    )
                )));
            }
        }
    }
    #[test]
    fn legacy_draw_keeps_premultiplied_panel_and_work_area_placement() {
        let now = Instant::now();
        let text = FakeText;
        let mut model = Indicator::new(&Look::WINDOWS_LEGACY);
        model.apply(
            Command::SetState(IndicatorState::Processing),
            &cx(now, &text),
        );
        settled(&mut model, now, &text);
        let frame = model.prepare_frame(&cx(now, &text)).unwrap();
        assert_eq!(frame.bounds, PxRect::new(615, 776, 690, 282));
        let mut canvas = RecordingCanvas::new(144);
        model.draw(&mut canvas, now);
        assert!(matches!(canvas.calls[0], Draw::FillRounded(_, PILL_RADIUS, c)
            if c == Look::WINDOWS_LEGACY.color(23.0/255.0, 24.0/255.0, 27.0/255.0, 0.96)));
        assert!(matches!(canvas.calls[1], Draw::StrokeRounded(_, _, _, _)));
        assert!(!matches!(canvas.calls[2], Draw::StrokeRounded(_, _, _, _)));
        assert!(!canvas.calls.iter().any(|call| matches!(call, Draw::Text(..))));
    }

    /// 处理中的点阵铺满胶囊，但任何一个点都不能碰到或越过内缩的胶囊遮罩。
    #[test]
    fn scan_dots_stay_inside_the_inset_pill_mask() {
        let now = Instant::now();
        let text = FakeText;
        let mut model = Indicator::new(&Look::WINDOWS_LEGACY);
        model.apply(
            Command::SetState(IndicatorState::Processing),
            &cx(now, &text),
        );
        settled(&mut model, now, &text);
        let pill = model.pill_rect();
        let (cx_, cy) = ((pill.left + pill.right) / 2.0, (pill.top + pill.bottom) / 2.0);
        let mask_r = PILL_H / 2.0 - DOT_MASK_INSET;
        let straight = pill.width() / 2.0 - PILL_H / 2.0;
        for step in 0..12 {
            model.on_frame(0.05, &cx(now + Duration::from_millis(400 + step * 50), &text));
            let mut canvas = RecordingCanvas::new(144);
            model.draw(&mut canvas, now);
            let dots: Vec<_> = canvas
                .calls
                .iter()
                .filter_map(|call| match call {
                    Draw::FillCircle(center, radius, _) => Some((*center, *radius)),
                    _ => None,
                })
                .collect();
            assert!(dots.len() > 100, "点阵应铺满胶囊");
            for (center, radius) in dots {
                let dx = ((center.x - cx_).abs() - straight).max(0.0);
                let distance = (dx * dx + (center.y - cy).powi(2)).sqrt();
                assert!(distance + radius <= mask_r + 1e-3, "点越过了遮罩：{center:?}");
            }
        }
    }

    /// 亮带在两端折返时，相邻两帧的点阵几乎一样，不会因为尾巴换边而闪一下。
    #[test]
    fn scan_turnaround_is_continuous() {
        let now = Instant::now();
        let text = FakeText;
        let mut model = Indicator::new(&Look::WINDOWS_LEGACY);
        model.apply(
            Command::SetState(IndicatorState::Processing),
            &cx(now, &text),
        );
        settled(&mut model, now, &text);
        let mut radii = |t: f32| {
            model.scan_t = t;
            let mut canvas = RecordingCanvas::new(144);
            model.draw(&mut canvas, now);
            canvas
                .calls
                .iter()
                .filter_map(|call| match call {
                    Draw::FillCircle(_, radius, _) => Some(*radius),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        let turn = std::f32::consts::FRAC_PI_2 / SCAN_SPEED;
        let before = radii(turn - 0.008);
        let after = radii(turn + 0.008);
        assert_eq!(before.len(), after.len());
        let jump = before
            .iter()
            .zip(&after)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0, f32::max);
        assert!(jump < 0.05, "折返时点阵跳变：{jump}");
    }

    #[test]
    fn only_smart_processing_is_clickable_and_click_skips() {
        let now = Instant::now();
        let text = FakeText;
        let mut model = Indicator::new(&Look::WINDOWS_LEGACY).clickable();
        let ops = model.apply(
            Command::SetState(IndicatorState::Recording),
            &cx(now, &text),
        );
        assert!(!ops.contains(&WindowOp::SetInteractive(true)));
        let ops = model.apply(
            Command::SetState(IndicatorState::SmartProcessing),
            &cx(now, &text),
        );
        assert!(ops.contains(&WindowOp::SetInteractive(true)));
        settled(&mut model, now, &text);
        model.prepare_frame(&cx(now, &text));

        let hit = <Indicator as OverlayModel<FakeText>>::hit(&model);
        let pill = model.pill_rect();
        let scale = 144.0 / 96.0;
        let inside = (
            (((pill.left + pill.right) / 2.0) * scale) as i32,
            (((pill.top + pill.bottom) / 2.0) * scale) as i32,
        );
        let mut pointer = IndicatorPointer::default();
        let steps = pointer.event(
            PointerEvent::Move { client: inside, screen: inside },
            &hit,
        );
        assert!(steps.contains(&PointerStep::Model(IndicatorInput::Hover(true))));
        assert_eq!(pointer.cursor(&hit), CursorShape::Hand);
        model.input(IndicatorInput::Hover(true), &cx(now, &text));
        assert_eq!(model.pill_target(), PILL_W_HOVER);
        pointer.event(PointerEvent::LeftDown { client: inside, screen: inside }, &hit);
        assert_eq!(
            pointer.event(PointerEvent::LeftUp { client: inside }, &hit),
            vec![
                PointerStep::Model(IndicatorInput::Skip),
                PointerStep::Op(WindowOp::Action(IndicatorAction::SkipSmartProcessing)),
            ]
        );
        // 点击后立即退出可点击状态，不必等应用回传。
        let ops = model.input(IndicatorInput::Skip, &cx(now, &text));
        assert!(ops.contains(&WindowOp::SetInteractive(false)));
        assert_eq!(model.state, Some(IndicatorState::Processing));
        let ops = model.apply(
            Command::SetState(IndicatorState::SmartProcessing),
            &cx(now, &text),
        );
        assert!(ops.contains(&WindowOp::SetInteractive(true)));
        let hit = <Indicator as OverlayModel<FakeText>>::hit(&model);
        // 点在胶囊外不触发。
        pointer.event(PointerEvent::LeftDown { client: (2, 2), screen: (2, 2) }, &hit);
        assert!(pointer.event(PointerEvent::LeftUp { client: (2, 2) }, &hit).is_empty());

        let ops = model.apply(
            Command::SetState(IndicatorState::Processing),
            &cx(now, &text),
        );
        assert!(ops.contains(&WindowOp::SetInteractive(false)));
        assert!(<Indicator as OverlayModel<FakeText>>::hit(&model).skip.is_none());
        assert_eq!(model.pill_target(), PILL_W_PROCESSING);

        // 宿主没接通指针（macOS）时始终点击穿透。
        let mut passive = Indicator::new(&Look::WINDOWS_LEGACY);
        let ops = passive.apply(
            Command::SetState(IndicatorState::SmartProcessing),
            &cx(now, &text),
        );
        assert!(!ops.contains(&WindowOp::SetInteractive(true)));
    }

    /// 波形按帧连续左移，固定周期出一根柱，周期内的音频块取平均。
    #[test]
    fn waveform_scrolls_continuously_and_averages_chunks_per_bar() {
        let now = Instant::now();
        let text = FakeText;
        let mut model = Indicator::new(&Look::WINDOWS_LEGACY);
        model.apply(
            Command::SetState(IndicatorState::Recording),
            &cx(now, &text),
        );
        for rms in [0.05, 0.15] {
            model.apply(
                Command::SetWaveform { level: rms, peaks: vec![rms; 6] },
                &cx(now, &text),
            );
        }
        model.on_frame(0.016, &cx(now + Duration::from_millis(16), &text));
        assert!(model.wave_scroll > 0.05 && model.wave_scroll < 0.15);
        model.on_frame(0.15, &cx(now + Duration::from_millis(166), &text));
        let expected = (bar_level(0.05) + bar_level(0.15)) / 2.0 * (1.0 - WAVE_NEIGHBOR_SMOOTH);
        assert!((model.wave_bars.back().copied().unwrap() - expected).abs() < 1e-5);
        assert!(model.wave_scroll < 0.1);
    }

    /// 整排都静音时波形停住，开口后继续滚动。
    #[test]
    fn waveform_holds_still_during_silence() {
        let now = Instant::now();
        let text = FakeText;
        let mut model = Indicator::new(&Look::WINDOWS_LEGACY);
        model.apply(
            Command::SetState(IndicatorState::Recording),
            &cx(now, &text),
        );
        for step in 1..=10 {
            model.apply(
                Command::SetWaveform { level: 0.001, peaks: vec![0.001; 6] },
                &cx(now, &text),
            );
            model.on_frame(0.1, &cx(now + Duration::from_millis(step * 100), &text));
        }
        assert_eq!(model.wave_scroll, 1.0);
        assert!(model.wave_bars.iter().all(|bar| *bar == 0.0));
        model.apply(
            Command::SetWaveform { level: 0.1, peaks: vec![0.1; 6] },
            &cx(now, &text),
        );
        model.on_frame(0.016, &cx(now + Duration::from_millis(1016), &text));
        assert!(model.wave_bars.back().copied().unwrap() > WAVE_SILENT);
        assert!(model.wave_scroll < 0.2);
    }

    #[test]
    fn css_uses_straight_alpha_and_glass_matches_only_visible_panels() {
        use crate::look::{Glass, GlassMaterial};
        let look = Look {
            glass: Some(Glass {
                material: GlassMaterial::UnderWindow,
                tint_percent: 20,
                border_percent: 30,
            }),
            ..Look::MACOS_DEFAULT
        };
        let now = Instant::now();
        let text = FakeText;
        let mut model = Indicator::new(&look);
        let shapes = |model: &Indicator| <Indicator as OverlayModel<FakeText>>::glass(model);
        assert!(shapes(&model).is_empty());
        for state in [
            IndicatorState::Recording,
            IndicatorState::Processing,
            IndicatorState::SmartProcessing,
            IndicatorState::Fallback,
            IndicatorState::Notice,
        ] {
            model.apply(Command::SetState(state), &cx(now, &text));
            for value in ["", "这是一段测试文本"] {
                model.apply(
                    Command::SetText {
                        text: value.into(),
                        fade: false,
                    },
                    &cx(now, &text),
                );
                let mut canvas = RecordingCanvas::new(144);
                model.draw(&mut canvas, now);
                let panels: Vec<_> = canvas
                    .calls
                    .iter()
                    .filter_map(|call| match call {
                        Draw::FillRounded(rect, radius, _)
                            if *radius == PILL_RADIUS || *radius == TEXT_RADIUS =>
                        {
                            Some(Shape::RoundedRect {
                                rect: *rect,
                                radius: *radius,
                            })
                        }
                        _ => None,
                    })
                    .collect();
                let mut glass = shapes(&model);
                // 绘制按文本框→胶囊，遮罩的并集与顺序无关。
                glass.reverse();
                assert_eq!(glass, panels);
            }
        }
        model.apply(
            Command::SetState(IndicatorState::Processing),
            &cx(now, &text),
        );
        model.apply(
            Command::SetText {
                text: String::new(),
                fade: false,
            },
            &cx(now, &text),
        );
        let mut canvas = RecordingCanvas::new(144);
        model.draw(&mut canvas, now);
        assert!(matches!(canvas.calls[0], Draw::FillRounded(_, _, color)
            if color == Color::new(23.0 / 255.0, 24.0 / 255.0, 27.0 / 255.0, 0.96)));
        <Indicator as OverlayModel<FakeText>>::set_look(&mut model, &Look::MACOS_DEFAULT);
        assert!(shapes(&model).is_empty());
        <Indicator as OverlayModel<FakeText>>::set_look(
            &mut model,
            &Look {
                glass: look.glass,
                ..Look::WINDOWS_LEGACY
            },
        );
        assert!(shapes(&model).is_empty());
    }

    #[test]
    fn wave_scale_matches_the_perceptual_loudness_curve() {
        assert_eq!(wave_scale(f32::NAN), 0.0);
        assert_eq!(wave_scale(-1.0), 0.0);
        assert_eq!(wave_scale(0.0), 0.0);
        assert_eq!(wave_scale(1.0), 1.0);
        assert_eq!(wave_scale(2.0), 1.0);
        assert!((wave_scale(0.25) - 0.9).abs() < 1e-6);
    }

}
