//! 听写胶囊的状态、动画与绘制；窗口副作用由宿主按序执行。
use crate::anim::{ease_out_cubic, Transition};
use crate::canvas::{
    Canvas, FontFamily, ParagraphAlign, TextAlign, TextEngine, TextStyle, TextTint,
};
use crate::geom::{Color, Point, PxRect, Rect, Stroke};
use crate::look::Look;
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
const PILL_W: f32 = 128.0;
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
const DOT_RADIUS: f32 = 5.0;
const DOT_LABEL_GAP: f32 = 8.0;

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
/// 状态点颜色过渡时长（recording 红 → processing 蓝等状态切换）。
const DOT_BLEND_S: f32 = 0.15;

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

    fn label(self) -> &'static str {
        match self {
            Self::Recording => "聆听中…",
            Self::Processing => "识别中…",
            Self::SmartProcessing => "处理中…",
            Self::Fallback => "",
            // Notice 不显示胶囊标签，内容走通知面板。
            Self::Notice => "",
        }
    }
}

pub struct Indicator {
    state: Option<IndicatorState>,
    text: String,
    /// 文本中「新增内容」的起始字符索引，配合 fresh_started 做淡入。
    fresh_from: usize,
    fresh_started: Option<Instant>,
    /// 状态点呼吸相位（秒），随帧推进。
    pulse_time: f32,
    /// 平滑后的波形柱高（CSS 的 70ms ease-out height 过渡等效）。
    wave_display: [f32; WAVE_BAR_COUNT],

    wave_active: bool,
    wave_level: f32,
    wave_peaks: Vec<f32>,
    /// Notice 态的自定义文案。
    notice_text: String,
    /// 状态点颜色的 150ms 过渡：from → target。
    dot_color_from: [f32; 3],
    dot_color_target: [f32; 3],
    dot_blend_start: Option<Instant>,
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
            pulse_time: 0.0,
            wave_display: [0.0; WAVE_BAR_COUNT],
            wave_active: false,
            wave_level: 0.0,
            wave_peaks: Vec::new(),
            notice_text: String::new(),
            dot_color_from: dot_target_color(IndicatorState::Recording),
            dot_color_target: dot_target_color(IndicatorState::Recording),
            dot_blend_start: None,
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
    /// 每帧推进：呼吸相位、波形平滑、文本淡入与点色过渡的收尾。
    fn tick_animations(&mut self, dt: f32, now: Instant) {
        self.pulse_time += dt;
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
        if self
            .dot_blend_start
            .is_some_and(|start| now.saturating_duration_since(start).as_secs_f32() >= DOT_BLEND_S)
        {
            self.dot_blend_start = None;
        }
    }

    /// 当前状态点颜色（含过渡中的插值）。
    fn dot_color_now(&self, now: Instant) -> [f32; 3] {
        let Some(start) = self.dot_blend_start else {
            return self.dot_color_target;
        };
        let elapsed = now.saturating_duration_since(start).as_secs_f32();
        let t = (elapsed / DOT_BLEND_S).clamp(0.0, 1.0);
        let eased = ease_out_cubic(t);
        let from = self.dot_color_from;
        let to = self.dot_color_target;
        [
            from[0] + (to[0] - from[0]) * eased,
            from[1] + (to[1] - from[1]) * eased,
            from[2] + (to[2] - from[2]) * eased,
        ]
    }

    /// 状态点的呼吸：CSS pulse（录音 1s，scale 1→0.6，opacity 1→0.5）与
    /// processingPulse（1.35s，scale 0.88→1.08，opacity 0.72→1）。
    fn dot_pulse(&self, state: IndicatorState) -> (f32, f32) {
        let period = if state == IndicatorState::Recording {
            1.0
        } else {
            1.35
        };
        let osc = 0.5 - 0.5 * (std::f32::consts::TAU * self.pulse_time / period).cos();
        if state == IndicatorState::Recording {
            (1.0 - 0.4 * osc, 1.0 - 0.5 * osc)
        } else {
            (0.88 + 0.20 * osc, 0.72 + 0.28 * osc)
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
                let target_color = dot_target_color(state);
                if target_color != self.dot_color_target {
                    self.dot_color_from = self.dot_color_now(cx.now);
                    self.dot_color_target = target_color;
                    self.dot_blend_start = Some(cx.now);
                }
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
    fn wants_frames(&self, _: Instant) -> bool {
        self.state.is_some() || self.transition.is_animating()
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
        self.fill_rounded(
            canvas,
            pill,
            PILL_RADIUS,
            self.look
                .color(12.0 / 255.0, 16.0 / 255.0, 24.0 / 255.0, 0.94),
            self.look.color(1.0, 1.0, 1.0, 0.16),
        );
        if state == IndicatorState::Recording {
            self.draw_waveform(canvas, pill);
        } else {
            self.draw_pill_face(canvas, state, pill, now);
        }
    }
    fn draw_text_box<C: Canvas>(&self, canvas: &mut C, rect: Rect, now: Instant) {
        self.fill_rounded(
            canvas,
            rect,
            TEXT_RADIUS,
            self.look
                .color(12.0 / 255.0, 16.0 / 255.0, 24.0 / 255.0, 0.96),
            self.look.color(1.0, 1.0, 1.0, 0.16),
        );
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
                color: self.look.color(234.0 / 255.0, 240.0 / 255.0, 1.0, fade),
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
            self.look.color(234.0 / 255.0, 240.0 / 255.0, 1.0, 1.0),
            tint,
        );
        canvas.restore();
    }
    fn draw_pill_face<C: Canvas>(
        &self,
        canvas: &mut C,
        state: IndicatorState,
        rect: Rect,
        now: Instant,
    ) {
        let label = state.label();
        let style = text_style(13.0, true);
        let label_w = canvas
            .text()
            .layout(label, &style, 1_000.0, 100.0)
            .map(|layout| canvas.text().metrics(&layout).width)
            .unwrap_or(0.0);
        let total = DOT_RADIUS * 2.0 + DOT_LABEL_GAP + label_w;
        let center_x = (rect.left + rect.right) / 2.0;
        let center_y = (rect.top + rect.bottom) / 2.0;
        let [cr, cg, cb] = self.dot_color_now(now);
        let (dot_scale, dot_alpha) = self.dot_pulse(state);
        let radius = DOT_RADIUS * dot_scale;
        canvas.fill_circle(
            Point::new(center_x - total / 2.0 + DOT_RADIUS, center_y),
            radius,
            self.look.color(cr, cg, cb, dot_alpha),
        );
        let bounds = Rect::new(
            center_x - total / 2.0 + DOT_RADIUS * 2.0 + DOT_LABEL_GAP,
            rect.top,
            rect.right,
            rect.bottom,
        );
        if let Some(layout) = canvas
            .text()
            .layout(label, &style, bounds.width(), bounds.height())
        {
            canvas.draw_text(
                &layout,
                Point::new(bounds.left, bounds.top),
                self.look.color(1.0, 1.0, 1.0, 1.0),
                None,
            );
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
                self.look.color(118.0 / 255.0, 167.0 / 255.0, 1.0, 1.0),
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
        let (bg, border) = if warn {
            (
                self.look
                    .color(32.0 / 255.0, 22.0 / 255.0, 14.0 / 255.0, 0.97),
                self.look.color(1.0, 171.0 / 255.0, 92.0 / 255.0, 0.5),
            )
        } else {
            (
                self.look
                    .color(12.0 / 255.0, 24.0 / 255.0, 42.0 / 255.0, 0.97),
                self.look.color(109.0 / 255.0, 174.0 / 255.0, 1.0, 0.5),
            )
        };
        self.fill_rounded(canvas, rect, TEXT_RADIUS, bg, border);
        let color = if warn {
            self.look.color(1.0, 171.0 / 255.0, 92.0 / 255.0, 1.0)
        } else {
            self.look.color(109.0 / 255.0, 174.0 / 255.0, 1.0, 1.0)
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
                self.look.color(234.0 / 255.0, 242.0 / 255.0, 1.0, 1.0),
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

/// 状态点目标色（直线 RGB 0..1）：recording 红 / 其余蓝，与 indicator.css 一致。
fn dot_target_color(state: IndicatorState) -> [f32; 3] {
    if state == IndicatorState::Recording {
        [1.0, 77.0 / 255.0, 79.0 / 255.0]
    } else {
        [109.0 / 255.0, 174.0 / 255.0, 1.0]
    }
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
            if c == Look::WINDOWS_LEGACY.color(12.0/255.0, 16.0/255.0, 24.0/255.0, 0.94)));
        assert!(matches!(canvas.calls[1], Draw::StrokeRounded(_, _, _, _)));
        assert!(matches!(canvas.calls[2], Draw::FillCircle(_, _, _)));
        assert!(matches!(canvas.calls[3], Draw::Text(_, _, _, None)));
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
