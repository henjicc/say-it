//! 听写胶囊的状态、动画与绘制；窗口副作用由宿主按序执行。
use crate::anim::{ease_out_cubic, Transition};
use crate::canvas::{
    Canvas, FontFamily, ParagraphAlign, TextAlign, TextEngine, TextStyle, TextTint,
};
use crate::geom::{Color, Point, PxRect, Rect, Shape, Stroke};
use crate::look::{Look, Profile};
use crate::window::{Cx, FrameSpec, NoPointer, Ops, OverlayModel, WindowEvent, WindowOp};
use std::convert::Infallible;
use std::time::{Duration, Instant};

// 与 indicator.rs 的 DEFAULT_INDICATOR_WIDTH/HEIGHT 保持一致。
const LOGICAL_WIDTH: f32 = 460.0;
const LOGICAL_HEIGHT: f32 = 188.0;
// 距主显示器工作区底边的逻辑像素。窗口底部自带 24px 透明内边距，
// -12 让可见内容最终停在任务栏上方约 12px（与 macOS 相对 Dock 的取值同理）。
const OFFSET_Y: f32 = -12.0;

// 以下几何与 indicator.css 的 dictation-mode 一一对应。
const WRAP_PADDING_BOTTOM: f32 = 24.0;
const STACK_GAP: f32 = 10.0;
const TEXT_W: f32 = 396.0;
const TEXT_H: f32 = 85.5; // 22.5 * 3 + 18
const TEXT_PAD: f32 = 12.0;
const TEXT_PAD_Y: f32 = 9.0;
const TEXT_VISIBLE_H: f32 = 67.5; // 22.5 * 3
const TEXT_RADIUS: f32 = 12.0;
const PILL_W: f32 = 168.0;
const PILL_H: f32 = 40.0;
const PILL_RADIUS: f32 = 20.0;
const FALLBACK_W: f32 = 396.0;
const FALLBACK_H: f32 = 56.0;
const FALLBACK_PAD: f32 = 14.0;
const FALLBACK_ICON: f32 = 18.0;
const FALLBACK_ICON_GAP: f32 = 10.0;
const WAVE_AREA_W: f32 = 80.0;
const WAVE_AREA_H: f32 = 26.0;
const WAVE_BAR_RATIO: f32 = 0.03; // OrbWaveform dense 变体
const WAVE_GAP_RATIO: f32 = 0.07;
/// 处理中胶囊里四段进度文字之间的间距。
const STAGE_GAP: f32 = 12.0;
/// 处理阶段：识别结束后依次经过智能处理、本地规则，再输出到光标。
const STAGES: [&str; 4] = ["识别", "处理", "规则", "输出"];

const NOTICE_AUTO_HIDE_MS: u32 = 3500;

/// 出场时从最终位置下方这么远（逻辑像素）淡入滑入；退场反向。
/// 与 indicator.css 的 wrapIn（translateY(10px)）一致。
const TRANSITION_SLIDE_PX: f32 = 10.0;

/// 文本新增内容的淡入时长，与 indicator.css 的 freshIn 180ms 一致。
const TEXT_FRESH_FADE_S: f32 = 0.18;
/// CSS 只给 ≤10 字符的短追加挂 .fresh 淡入；长追加直接显示。
const TEXT_FRESH_FADE_MAX_CHARS: usize = 10;
/// 波形柱高度平滑的时间常数，与 indicator.css 的 height 70ms ease-out 等效。
const WAVE_SMOOTH_S: f32 = 0.070;

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

    /// 处理中胶囊里当前高亮的阶段；其余状态不显示阶段。
    fn active_stage(self) -> Option<usize> {
        match self {
            Self::Processing => Some(0),
            Self::SmartProcessing => Some(1),
            _ => None,
        }
    }
}

pub struct Indicator {
    state: Option<IndicatorState>,
    text: String,
    /// 文本中「新增内容」的起始字符索引，配合 fresh_started 做淡入。
    fresh_from: usize,
    fresh_started: Option<Instant>,
    /// 平滑后的波形柱高（CSS 的 70ms ease-out height 过渡等效）。
    wave_display: [f32; WAVE_BAR_COUNT],

    wave_active: bool,
    wave_level: f32,
    wave_peaks: Vec<f32>,
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
            wave_display: [0.0; WAVE_BAR_COUNT],
            wave_active: false,
            wave_level: 0.0,
            wave_peaks: Vec::new(),
            notice_text: String::new(),
            dpi: 96,
            transition: Transition::new(false),
            notice_deadline: None,
            look: *look,
        }
    }
    fn clear_content(&mut self) {
        self.state = None;
        self.text.clear();
        self.wave_active = false;
        self.wave_peaks.clear();
    }
    fn begin_exit(&mut self) -> Ops<Infallible> {
        self.notice_deadline = None;
        if !self.transition.hide() {
            self.clear_content();
            vec![WindowOp::Hide]
        } else {
            Vec::new()
        }
    }
    /// 每帧推进：波形平滑与文本淡入的收尾。
    fn tick_animations(&mut self, dt: f32, now: Instant) {
        // 波形柱向目标值指数趋近，等效 CSS 的 70ms ease-out height 过渡。
        // 与 IndicatorApp.tsx 一致：每个峰值先过感知响度曲线再重采样；
        // 原始 RMS 响度正常说话只有 0.05~0.3，不过曲线柱高会常年压在最低线。
        let scaled_peaks: Vec<f32> = self.wave_peaks.iter().map(|v| wave_scale(*v)).collect();
        let target = resample_wave_levels(&scaled_peaks, wave_scale(self.wave_level));
        let k = 1.0 - (-dt / WAVE_SMOOTH_S).exp();
        for (display, target) in self.wave_display.iter_mut().zip(target) {
            *display += (target - *display) * k;
        }
        if self.fresh_started.is_some_and(|start| {
            now.saturating_duration_since(start).as_secs_f32() >= TEXT_FRESH_FADE_S
        }) {
            self.fresh_started = None;
        }
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
}

impl<T: TextEngine> OverlayModel<T> for Indicator {
    type Command = Command;
    type Action = Infallible;
    type Pointer = NoPointer;

    fn apply(&mut self, command: Command, cx: &Cx<'_, T>) -> Ops<Infallible> {
        match command {
            Command::Prepare => {
                self.notice_deadline = None;
                self.text.clear();
                self.fresh_from = 0;
                self.fresh_started = None;
                self.wave_active = false;
                self.wave_peaks.clear();
                self.wave_level = 0.0;
                Vec::new()
            }
            Command::SetState(state) => {
                self.notice_deadline = None;
                self.state = Some(state);
                if state == IndicatorState::Fallback {
                    self.text.clear();
                    self.wave_active = false;
                    self.wave_peaks.clear();
                }
                self.transition.show();
                vec![WindowOp::Redraw, WindowOp::RaiseTopmost]
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
                if self.state.is_some() {
                    vec![WindowOp::Redraw]
                } else {
                    Vec::new()
                }
            }
            Command::SetWaveform { level, peaks } => {
                self.wave_active = true;
                self.wave_level = level;
                self.wave_peaks = peaks;
                Vec::new()
            }
            Command::ShowNotice(text) => {
                self.text.clear();
                self.fresh_from = 0;
                self.fresh_started = None;
                self.wave_active = false;
                self.wave_peaks.clear();
                self.notice_text = text;
                self.state = Some(IndicatorState::Notice);
                self.transition.show();
                self.notice_deadline =
                    Some(cx.now + Duration::from_millis(NOTICE_AUTO_HIDE_MS as u64));
                vec![WindowOp::Redraw, WindowOp::RaiseTopmost]
            }
            Command::Hide => self.begin_exit(),
        }
    }
    fn input(&mut self, input: Infallible, _: &Cx<'_, T>) -> Ops<Infallible> {
        match input {}
    }
    fn window_event(&mut self, event: WindowEvent, cx: &Cx<'_, T>) -> Ops<Infallible> {
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
    fn set_look(&mut self, look: &Look) -> Ops<Infallible> {
        self.look = *look;
        vec![WindowOp::Redraw]
    }
    fn on_frame(&mut self, dt: f32, cx: &Cx<'_, T>) -> Ops<Infallible> {
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
    /// 只有波形、文字淡入、通知倒计时和入退场需要逐帧推进；处理中与兜底是静态画面。
    fn wants_frames(&self, _: Instant) -> bool {
        self.transition.is_animating()
            || self.state == Some(IndicatorState::Recording)
            || self.fresh_started.is_some()
            || self.notice_deadline.is_some()
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
    fn hit(&self) {}
    fn glass(&self) -> Vec<Shape> {
        if self.look.profile != Profile::Css || self.look.glass.is_none() {
            return Vec::new();
        }
        let Some(state) = self.state else {
            return Vec::new();
        };
        let bottom = LOGICAL_HEIGHT - WRAP_PADDING_BOTTOM;
        if matches!(state, IndicatorState::Fallback | IndicatorState::Notice) {
            return vec![Shape::RoundedRect {
                rect: Rect::new(
                    (LOGICAL_WIDTH - FALLBACK_W) / 2.0,
                    bottom - FALLBACK_H,
                    (LOGICAL_WIDTH + FALLBACK_W) / 2.0,
                    bottom,
                ),
                radius: TEXT_RADIUS,
            }];
        }
        let pill = Rect::new(
            (LOGICAL_WIDTH - PILL_W) / 2.0,
            bottom - PILL_H,
            (LOGICAL_WIDTH + PILL_W) / 2.0,
            bottom,
        );
        let mut shapes = vec![Shape::RoundedRect {
            rect: pill,
            radius: PILL_RADIUS,
        }];
        if !self.text.is_empty() {
            let bottom = pill.top - STACK_GAP;
            shapes.push(Shape::RoundedRect {
                rect: Rect::new(
                    (LOGICAL_WIDTH - TEXT_W) / 2.0,
                    bottom - TEXT_H,
                    (LOGICAL_WIDTH + TEXT_W) / 2.0,
                    bottom,
                ),
                radius: TEXT_RADIUS,
            });
        }
        shapes
    }
}

impl Indicator {
    fn fill_rounded<C: Canvas>(
        &self,
        canvas: &mut C,
        rect: Rect,
        radius: f32,
        fill: Color,
        border: Color,
    ) {
        canvas.fill_rounded_rect(rect, radius, fill);
        canvas.stroke_rounded_rect(rect, radius, border, Stroke::plain(1.0));
    }
    /// 胶囊、文本框与通知面板共用的底色与细边框，与主窗口检查器同色系。
    fn panel_fill(&self) -> Color {
        self.look.color(23.0 / 255.0, 24.0 / 255.0, 27.0 / 255.0, 0.96)
    }
    fn panel_border(&self) -> Color {
        self.look.color(1.0, 1.0, 1.0, 0.08)
    }
    fn draw_content<C: Canvas>(&self, canvas: &mut C, now: Instant) {
        let Some(state) = self.state else { return };
        let content_bottom = LOGICAL_HEIGHT - WRAP_PADDING_BOTTOM;
        if state == IndicatorState::Fallback {
            self.draw_notice_panel(
                canvas,
                content_bottom,
                "已经把结果放到你的剪贴板里了，你粘贴就可以用了",
                false,
            );
            return;
        }
        if state == IndicatorState::Notice {
            self.draw_notice_panel(canvas, content_bottom, &self.notice_text, true);
            return;
        }
        let pill = Rect::new(
            (LOGICAL_WIDTH - PILL_W) / 2.0,
            content_bottom - PILL_H,
            (LOGICAL_WIDTH + PILL_W) / 2.0,
            content_bottom,
        );
        if !self.text.is_empty() {
            let text_bottom = pill.top - STACK_GAP;
            self.draw_text_box(
                canvas,
                Rect::new(
                    (LOGICAL_WIDTH - TEXT_W) / 2.0,
                    text_bottom - TEXT_H,
                    (LOGICAL_WIDTH + TEXT_W) / 2.0,
                    text_bottom,
                ),
                now,
            );
        }
        self.fill_rounded(canvas, pill, PILL_RADIUS, self.panel_fill(), self.panel_border());
        if state == IndicatorState::Recording {
            self.draw_waveform(canvas, pill);
        } else if let Some(active) = state.active_stage() {
            self.draw_stages(canvas, active, pill);
        }
    }
    fn draw_text_box<C: Canvas>(&self, canvas: &mut C, rect: Rect, now: Instant) {
        self.fill_rounded(canvas, rect, TEXT_RADIUS, self.panel_fill(), self.panel_border());
        let inner_w = TEXT_W - TEXT_PAD * 2.0;
        let Some(layout) =
            canvas
                .text()
                .layout(&self.text, &text_style(14.0, false), inner_w, 10_000.0)
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
                color: fg(&self.look, fade),
            })
        } else {
            None
        };
        let metrics = canvas.text().metrics(&layout);
        let inner = Rect::new(
            rect.left + TEXT_PAD,
            rect.top + TEXT_PAD_Y,
            rect.right - TEXT_PAD,
            rect.top + TEXT_PAD_Y + TEXT_VISIBLE_H,
        );
        let offset = (metrics.height - TEXT_VISIBLE_H).max(0.0);
        canvas.save();
        canvas.clip_rect(inner);
        canvas.draw_text(
            &layout,
            Point::new(inner.left, inner.top - offset),
            fg(&self.look, 1.0),
            tint,
        );
        canvas.restore();
    }
    /// 处理中：胶囊里依次排列四个阶段，已完成为前景色，当前为强调色，未到为暗色。
    fn draw_stages<C: Canvas>(&self, canvas: &mut C, active: usize, pill: Rect) {
        let style = text_style(12.0, false);
        let layouts: Vec<_> = STAGES
            .iter()
            .map(|label| canvas.text().layout(label, &style, 1_000.0, 100.0))
            .collect();
        let widths: Vec<f32> = layouts
            .iter()
            .map(|layout| layout.as_ref().map_or(0.0, |l| canvas.text().metrics(l).width))
            .collect();
        let total = widths.iter().sum::<f32>() + STAGE_GAP * (STAGES.len() - 1) as f32;
        let center_y = (pill.top + pill.bottom) / 2.0;
        let mut x = (pill.left + pill.right - total) / 2.0;
        for (index, layout) in layouts.iter().enumerate() {
            if let Some(layout) = layout {
                let height = canvas.text().metrics(layout).height;
                let color = if index < active {
                    fg(&self.look, 1.0)
                } else if index == active {
                    self.look.color(113.0 / 255.0, 159.0 / 255.0, 219.0 / 255.0, 1.0)
                } else {
                    self.look.color(92.0 / 255.0, 95.0 / 255.0, 102.0 / 255.0, 1.0)
                };
                canvas.draw_text(layout, Point::new(x, center_y - height / 2.0), color, None);
            }
            x += widths[index] + STAGE_GAP;
        }
    }
    fn draw_waveform<C: Canvas>(&self, canvas: &mut C, pill: Rect) {
        let area_left = pill.left + (PILL_W - WAVE_AREA_W) / 2.0;
        let area_top = pill.top + (PILL_H - WAVE_AREA_H) / 2.0;
        let dpr = self.dpi as f32 / 96.0;
        let dpr = if dpr > 0.0 { dpr } else { 1.0 };
        let bar = ((WAVE_AREA_W * dpr * WAVE_BAR_RATIO).round() as i32).max(1) as f32 / dpr;
        let gap = ((WAVE_AREA_W * dpr * WAVE_GAP_RATIO).round() as i32).max(1) as f32 / dpr;
        let total = WAVE_BAR_COUNT as f32 * bar + (WAVE_BAR_COUNT - 1) as f32 * gap;
        let start = area_left + ((WAVE_AREA_W * dpr - total * dpr) / 2.0).round() / dpr;
        let levels = self.wave_display;
        for (index, level) in levels.iter().enumerate() {
            let scale = level.max(WAVE_BAR_MIN_SCALE);
            let mut height = WAVE_AREA_H * WAVE_BAR_HEIGHTS[index] * scale;
            height = height.max(bar);
            let left = start + index as f32 * (bar + gap);
            let top = area_top + (WAVE_AREA_H - height) / 2.0;
            canvas.fill_rounded_rect(
                Rect::new(left, top, left + bar, top + height),
                bar / 2.0,
                fg(&self.look, 1.0),
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
        self.fill_rounded(canvas, rect, TEXT_RADIUS, self.panel_fill(), self.panel_border());
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

/// 波形柱数量，与 IndicatorApp.tsx 的 INDICATOR_WAVE_BAR_COUNT 一致。
pub const WAVE_BAR_COUNT: usize = 9;

/// 每根波形柱的基础高度占比，与 indicator.css 的 --bar-height 逐个对应。
pub const WAVE_BAR_HEIGHTS: [f32; WAVE_BAR_COUNT] =
    [0.46, 0.58, 0.70, 0.82, 0.94, 0.82, 0.70, 0.58, 0.46];

/// 波形柱最小缩放，与 OrbWaveform 的 `max(0.18, level)` 一致。
pub const WAVE_BAR_MIN_SCALE: f32 = 0.18;

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

/// 把任意数量的峰值重采样到 9 根柱，移植自 IndicatorApp.tsx 的 resampleWaveLevels。
pub fn resample_wave_levels(levels: &[f32], fallback: f32) -> [f32; WAVE_BAR_COUNT] {
    if levels.is_empty() {
        return [fallback; WAVE_BAR_COUNT];
    }
    if levels.len() == 1 {
        return [levels[0]; WAVE_BAR_COUNT];
    }
    let mut out = [0.0; WAVE_BAR_COUNT];
    for (index, slot) in out.iter_mut().enumerate() {
        let position = index as f32 * (levels.len() - 1) as f32 / (WAVE_BAR_COUNT - 1) as f32;
        let lower = position.floor() as usize;
        let upper = (levels.len() - 1).min(position.ceil() as usize);
        let progress = position - lower as f32;
        *slot = levels[lower] + (levels[upper] - levels[lower]) * progress;
    }
    out
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
    fn legacy_draw_keeps_premultiplied_brush_order_and_work_area_placement() {
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
        assert!(matches!(canvas.calls[0], Draw::FillRounded(_, 20.0, c)
            if c == Look::WINDOWS_LEGACY.color(23.0/255.0, 24.0/255.0, 27.0/255.0, 0.96)));
        assert!(matches!(canvas.calls[1], Draw::StrokeRounded(_, _, _, _)));
        let stages = canvas.calls[2..]
            .iter()
            .filter(|call| matches!(call, Draw::Text(_, _, _, None)))
            .count();
        assert_eq!(stages, STAGES.len());
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

    #[test]
    fn resample_fills_fallback_when_no_levels() {
        assert_eq!(resample_wave_levels(&[], 0.5), [0.5; WAVE_BAR_COUNT]);
        assert_eq!(resample_wave_levels(&[0.3], 0.0), [0.3; WAVE_BAR_COUNT]);
    }

    #[test]
    fn resample_interpolates_like_the_webview() {
        let levels = [0.0, 1.0];
        let bars = resample_wave_levels(&levels, 0.0);
        assert_eq!(bars[0], 0.0);
        assert_eq!(bars[WAVE_BAR_COUNT - 1], 1.0);
        assert!((bars[4] - 0.5).abs() < 1e-6);

        let six = [0.0, 0.2, 0.4, 0.6, 0.8, 1.0];
        let bars = resample_wave_levels(&six, 0.0);
        // 6 → 9：position = index * 5 / 8，索引 4 落在 2.5 处。
        assert!((bars[4] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn wave_bar_height_table_matches_css() {
        assert_eq!(WAVE_BAR_HEIGHTS.len(), WAVE_BAR_COUNT);
        assert_eq!(WAVE_BAR_HEIGHTS[0], 0.46);
        assert_eq!(WAVE_BAR_HEIGHTS[4], 0.94);
        assert!(WAVE_BAR_MIN_SCALE > 0.0);
    }
}
