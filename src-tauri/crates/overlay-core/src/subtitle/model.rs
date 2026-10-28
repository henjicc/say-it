//! 字幕轨道、布局、动画与绘制；窗口操作由宿主在模型借用外执行。
use super::*;
use crate::anim::Transition;
use crate::canvas::*;
use crate::geom::{Color, Point, PxRect, Rect, Stroke, Transform};
use crate::look::Look;
use crate::window::*;
use std::time::Instant;

const SWAP_IN_S: f32 = 0.32; // indicator.css 的 swapIn
const CONTROLS_FADE_S: f32 = 0.12; // .subtitle-controls 的 opacity 过渡
/// UNIFORM 行距下基线在行框中的位置（贴近 CSS line-height 的居中效果）。
const BASELINE_RATIO: f32 = 0.8;

const BLOCK_GAP: f32 = 10.0;
const PAD_X: f32 = 22.0;
const PAD_TOP_MAIN: f32 = 14.0;
const PAD_TOP_TRANSLATION: f32 = 10.0;
/// 控制条：top 5 right 8，3 个 22px 按钮，gap 8，16px 图标。
const CONTROL_BUTTON: f32 = 22.0;
const CONTROL_GAP: f32 = 8.0;
const CONTROL_TOP: f32 = 5.0;
const CONTROL_RIGHT: f32 = 8.0;
const ICON_SIZE: f32 = 16.0;
const ICON_STROKE: f32 = 1.45;
const CONTROL_COUNT: usize = 3;
const BORDER: f32 = 1.0;

/// 出场/退场位移幅度，与 indicator.css 的 wrapIn（translateY(10px)）一致。
const TRANSITION_SLIDE_PX: f32 = 10.0;

// lucide 图标路径（24x24 视图盒，stroke 风格）。
const ICON_LOCK_SHACKLE: &str = "M7 11V7a5 5 0 0 1 10 0v4";
const ICON_UNLOCK_SHACKLE: &str = "M7 11V7a5 5 0 0 1 9.9-1";
const ICON_ROTATE: &str = "M21 12a9 9 0 1 1-9-9c2.52 0 4.93 1 6.74 2.74L21 8M21 3v5h-5";
const ICON_CLOSE: &str = "M18 6L6 18M6 6L18 18";

#[derive(Clone, Debug)]
pub enum Command {
    SetConfig(Box<SubtitleConfig>),
    SetLayout {
        width: f32,
        height: f32,
        anchor: String,
        offset_y: f32,
    },
    SetText {
        text: String,
        fade: bool,
    },
    SetTranslation(String),
    Show,
    Hide,
}

/// 一路文本轨道（useTextTrack 移植）：原文/译文各一份。
struct TextTrack<L> {
    displayed: String,
    layout: Option<L>,
    content_size: (f32, f32),
    /// 「新增内容」的起始字符索引；usize::MAX 表示本轮没有新增区间。
    fresh_from: usize,
    fresh_started: Option<Instant>,
    /// swapText（fade=true）：整段换新并播 swapIn。
    swap_started: Option<Instant>,
    offset: f32,
    offset_from: f32,
    offset_target: f32,
    offset_anim: Option<Instant>,
}

impl<L> Default for TextTrack<L> {
    fn default() -> Self {
        Self {
            displayed: String::new(),
            layout: None,
            content_size: (0.0, 0.0),
            fresh_from: usize::MAX,
            fresh_started: None,
            swap_started: None,
            offset: 0.0,
            offset_from: 0.0,
            offset_target: 0.0,
            offset_anim: None,
        }
    }
}

impl<L> TextTrack<L> {
    fn reset(&mut self) {
        *self = Self::default();
    }

    fn has_text(&self) -> bool {
        !self.displayed.is_empty()
    }

    fn animating(&self) -> bool {
        self.fresh_started.is_some() || self.swap_started.is_some() || self.offset_anim.is_some()
    }
}

/// 字幕内容、控件与逻辑尺寸；不持有平台绘制资源。
struct SubtitleView<L> {
    config: SubtitleConfig,
    // SetLayout 是窗口几何的权威；config 里的同名字段供「重置位置」恢复。
    layout_w: f32,
    layout_h: f32,
    anchor: String,
    offset_y: f32,
    locked: bool,
    tracks: [TextTrack<L>; 2],
    hovering_block: bool,
    hover_button: Option<usize>,
    controls_alpha: f32,
    controls_anim: Option<(Instant, f32, f32)>,
    dpi: u32,
}

impl<L: Clone> SubtitleView<L> {
    fn line_height(&self) -> f32 {
        line_height(self.config.font_size)
    }

    fn block_width(&self) -> f32 {
        // width: min(config.width, 窗口宽)
        (self.config.width as f32).min(self.layout_w)
    }

    fn flow_width(&self) -> f32 {
        (self.block_width() - (PAD_X + BORDER) * 2.0).max(1.0)
    }

    fn flow_height(&self) -> f32 {
        self.line_height() * self.config.line_count.max(1) as f32
    }

    fn is_replace(&self) -> bool {
        self.config.display_mode == "replace"
    }

    fn show_translation(&self) -> bool {
        // #translation-text.empty { display: none }：译文块只在双语且有内容时出现。
        self.config.translation_enabled
            && self.config.translation_layout == "bilingual"
            && self.tracks[1].has_text()
    }

    /// 当前应显示的块（轨道索引 + 矩形），自上而下；底部对齐、gap 10。
    fn blocks(&self) -> Vec<(usize, Rect)> {
        let line_h = self.line_height();
        let lines = self.config.line_count.max(1);
        let translation_first = self.config.translation_order == "translationFirst";
        let show_translation = self.show_translation();
        let mut order: Vec<(usize, f32)> = Vec::with_capacity(2);
        let original = (0usize, block_height(line_h, lines, false));
        let translation = (1usize, block_height(line_h, lines, true));
        if translation_first {
            if show_translation {
                order.push(translation);
            }
            order.push(original);
        } else {
            order.push(original);
            if show_translation {
                order.push(translation);
            }
        }
        let heights: Vec<f32> = order.iter().map(|(_, h)| *h).collect();
        let tops = stack_from_bottom(self.layout_h, &heights, BLOCK_GAP);
        let width = self.block_width();
        let left = (self.layout_w - width) / 2.0;
        order
            .iter()
            .zip(tops)
            .map(|((track, height), top)| {
                (*track, Rect::new(left, top, left + width, top + height))
            })
            .collect()
    }

    fn original_block_rect(&self) -> Rect {
        self.blocks()
            .into_iter()
            .find(|(track, _)| *track == 0)
            .map(|(_, rect)| rect)
            .unwrap_or_else(|| Rect::new(0.0, 0.0, 0.0, 0.0))
    }

    /// 控制条三个按钮的矩形（lock/reset/close，从左到右）。
    fn control_rects(&self) -> Option<[Rect; CONTROL_COUNT]> {
        // 只有原文有文本时才出现（IndicatorApp: original.hasText）。
        if !self.tracks[0].has_text() {
            return None;
        }
        let block = self.original_block_rect();
        let total =
            CONTROL_BUTTON * CONTROL_COUNT as f32 + CONTROL_GAP * (CONTROL_COUNT - 1) as f32;
        let left = block.right - CONTROL_RIGHT - total;
        let top = block.top + CONTROL_TOP;
        let mut rects = [Rect::new(0.0, 0.0, 0.0, 0.0); CONTROL_COUNT];
        for (index, rect) in rects.iter_mut().enumerate() {
            let x = left + index as f32 * (CONTROL_BUTTON + CONTROL_GAP);
            *rect = Rect::new(x, top, x + CONTROL_BUTTON, top + CONTROL_BUTTON);
        }
        Some(rects)
    }

    fn animate_controls(&mut self, target: f32, now: Instant) {
        if (self.controls_alpha - target).abs() < 0.001 && self.controls_anim.is_none() {
            return;
        }
        self.controls_anim = Some((now, self.controls_alpha, target));
    }

    /// 可视区变化时连同测量和滚动目标一起更新，避免沿用旧宽度/行数的布局。
    fn reflow_tracks<T: TextEngine<Layout = L>>(&mut self, text: &T, now: Instant) {
        for index in 0..2 {
            self.tracks[index].layout = None;
            self.update_track_offset(index, false, text, now);
        }
    }

    /// 为两条轨道建好文本布局（文本/字体变化后 layout 置空，渲染前重建）。
    fn prepare_layouts<T: TextEngine<Layout = L>>(&mut self, text: &T) {
        for index in 0..2 {
            if self.tracks[index].layout.is_some() || self.tracks[index].displayed.is_empty() {
                continue;
            }
            let Some(layout) = build_text_layout(
                text,
                &self.tracks[index].displayed.clone(),
                self.flow_width(),
                self.flow_height(),
                self.is_replace(),
                self.line_height(),
                &self.config,
            ) else {
                continue;
            };
            self.tracks[index].content_size = layout_size(text, &layout);
            self.tracks[index].layout = Some(layout);
        }
    }

    fn set_track_text<T: TextEngine<Layout = L>>(
        &mut self,
        index: usize,
        value: &str,
        fade: bool,
        text: &T,
        now: Instant,
    ) {
        let next = trim_render_text(value);
        if next.is_empty() {
            self.tracks[index].reset();
            if index == 0 {
                // 原文清空后控制条也失去存在条件。
                self.animate_controls(0.0, now);
            }
            return;
        }
        let was_empty = self.tracks[index].displayed.is_empty();
        {
            let track = &mut self.tracks[index];
            if fade {
                // swapText：整段换新，不做前缀 diff，播 swapIn。
                track.fresh_from = usize::MAX;
                track.fresh_started = None;
                track.swap_started = Some(now);
            } else {
                let overlap = overlap_chars(&track.displayed, &next);
                let fresh_len = next.chars().count().saturating_sub(overlap);
                track.fresh_from = overlap;
                track.fresh_started = (fresh_len > 0
                    && fresh_len <= FRESH_FADE_MAX_CHARS
                    && self.config.fade_enabled
                    && self.config.fade_duration_ms > 0)
                    .then_some(now);
                track.swap_started = None;
            }
            track.displayed = next;
            track.layout = None;
        }
        self.update_track_offset(index, was_empty, text, now);
    }

    /// 内容尺寸变化后更新位移目标；首个词出现时不播位移动画直接落位
    /// （对照 applyScroll(skipAnimation=true)）。
    fn update_track_offset<T: TextEngine<Layout = L>>(
        &mut self,
        index: usize,
        first_paint: bool,
        engine: &T,
        now: Instant,
    ) {
        if self.tracks[index].displayed.is_empty() {
            return;
        }
        // 测量需要先建布局。
        if self.tracks[index].layout.is_none() {
            let text = self.tracks[index].displayed.clone();
            let Some(layout) = build_text_layout(
                engine,
                &text,
                self.flow_width(),
                self.flow_height(),
                self.is_replace(),
                self.line_height(),
                &self.config,
            ) else {
                return;
            };
            self.tracks[index].content_size = layout_size(engine, &layout);
            self.tracks[index].layout = Some(layout);
        }
        let (content_w, content_h) = self.tracks[index].content_size;
        let target = if self.is_replace() {
            replace_offset_x(content_w, self.flow_width())
        } else {
            scroll_offset_y(content_h, self.flow_height())
        };
        let track = &mut self.tracks[index];
        if first_paint || !self.config.motion_enabled || self.config.motion_duration_ms == 0 {
            track.offset = target;
            track.offset_from = target;
            track.offset_target = target;
            track.offset_anim = None;
        } else if (target - track.offset_target).abs() > 0.5 {
            track.offset_from = track.offset;
            track.offset_target = target;
            track.offset_anim = Some(now);
        }
    }

    fn tick(&mut self, now: Instant) {
        for index in 0..2 {
            let fade_ms = self.config.fade_duration_ms.max(1) as f32;
            let motion_ms = self.config.motion_duration_ms.max(1) as f32;
            let track = &mut self.tracks[index];
            if track.fresh_started.is_some_and(|start| {
                now.saturating_duration_since(start).as_secs_f32() * 1000.0 >= fade_ms
            }) {
                track.fresh_started = None;
                // 淡入完成后重建布局，摘掉新增区间的画刷效果。
                track.layout = None;
            }
            if track.swap_started.is_some_and(|start| {
                now.saturating_duration_since(start).as_secs_f32() >= SWAP_IN_S
            }) {
                track.swap_started = None;
            }
            if let Some(start) = track.offset_anim {
                let elapsed = now.saturating_duration_since(start).as_secs_f32();
                let t = (elapsed * 1000.0 / motion_ms).min(1.0);
                let eased = ease_value(&self.config.motion_easing, t);
                track.offset =
                    track.offset_from + (track.offset_target - track.offset_from) * eased;
                if t >= 1.0 {
                    track.offset = track.offset_target;
                    track.offset_anim = None;
                }
            }
        }
        if let Some((start, from, to)) = self.controls_anim {
            let elapsed = now.saturating_duration_since(start).as_secs_f32();
            let t = (elapsed / CONTROLS_FADE_S).min(1.0);
            self.controls_alpha = from + (to - from) * ease_value("ease-out", t);
            if t >= 1.0 {
                self.controls_alpha = to;
                self.controls_anim = None;
            }
        }
    }

    fn draw<T: TextEngine<Layout = L>, C: Canvas<Text = T>>(&self, canvas: &mut C, now: Instant) {
        for (track, rect) in self.blocks() {
            self.draw_block(canvas, track, rect, now);
        }
        self.draw_controls(canvas);
    }

    fn draw_block<T: TextEngine<Layout = L>, C: Canvas<Text = T>>(
        &self,
        canvas: &mut C,
        index: usize,
        rect: Rect,
        now: Instant,
    ) {
        let translation = index == 1;
        let alpha_scale = if translation { 0.92 } else { 1.0 };
        let (bg_r, bg_g, bg_b, bg_a) =
            parse_css_color(&self.config.background_color).unwrap_or((5, 7, 10, 0.72));
        let radius = self.config.rounded as f32;
        canvas.fill_rounded_rect(
            rect,
            radius,
            Color::new(
                bg_r as f32 / 255.0,
                bg_g as f32 / 255.0,
                bg_b as f32 / 255.0,
                bg_a * alpha_scale,
            ),
        );
        canvas.stroke_rounded_rect(
            Rect::new(
                rect.left + 0.5,
                rect.top + 0.5,
                rect.right - 0.5,
                rect.bottom - 0.5,
            ),
            (radius - 0.5).max(0.0),
            Color::new(
                1.0,
                1.0,
                1.0,
                if translation {
                    0.10 * alpha_scale
                } else {
                    0.08
                },
            ),
            Stroke::plain(BORDER),
        );
        let track = &self.tracks[index];
        let (Some(layout), true) = (&track.layout, track.has_text()) else {
            return;
        };
        let pad_top = if translation {
            PAD_TOP_TRANSLATION
        } else {
            PAD_TOP_MAIN
        };
        let flow = Rect::new(
            rect.left + PAD_X + BORDER,
            rect.top + pad_top + BORDER,
            rect.right - PAD_X - BORDER,
            rect.top + pad_top + BORDER + self.flow_height(),
        );
        let (text_r, text_g, text_b, text_a) =
            parse_css_color(&self.config.text_color).unwrap_or((255, 255, 255, 1.0));
        let swap_alpha = track
            .swap_started
            .map(|start| {
                ease_value(
                    "ease-out",
                    (now.saturating_duration_since(start).as_secs_f32() / SWAP_IN_S).min(1.0),
                )
            })
            .unwrap_or(1.0);
        let base_alpha = text_a * alpha_scale * swap_alpha;
        let color = |a| {
            Color::new(
                text_r as f32 / 255.0,
                text_g as f32 / 255.0,
                text_b as f32 / 255.0,
                a,
            )
        };
        let tint = track.fresh_started.and_then(|start| {
            let fade = ease_value(
                &self.config.fade_easing,
                (now.saturating_duration_since(start).as_secs_f32() * 1000.0
                    / self.config.fade_duration_ms.max(1) as f32)
                    .min(1.0),
            );
            if track.fresh_from >= track.displayed.chars().count() {
                return None;
            }
            let utf16_start = track
                .displayed
                .chars()
                .take(track.fresh_from)
                .map(|c| c.len_utf16() as u32)
                .sum();
            let total = track.displayed.encode_utf16().count() as u32;
            (utf16_start < total).then_some(TextTint {
                utf16_start,
                utf16_len: total - utf16_start,
                color: color(base_alpha * fade),
            })
        });
        let (dx, dy) = if self.is_replace() {
            (track.offset, 0.0)
        } else {
            (0.0, -track.offset)
        };
        canvas.save();
        canvas.clip_rect(flow);
        let blurred = if swap_alpha < 0.999 {
            match canvas.draw_text_blurred(
                layout,
                flow,
                Point::new(dx, dy),
                color(base_alpha),
                6.0 * (1.0 - swap_alpha),
            ) {
                Ok(()) => true,
                Err(error) => {
                    eprintln!("[native-subtitle] 绘制字幕模糊失败，回退清晰文本：{error}");
                    false
                }
            }
        } else {
            false
        };
        if !blurred {
            canvas.draw_text(
                layout,
                Point::new(flow.left + dx, flow.top + dy),
                color(base_alpha),
                tint,
            );
        }
        canvas.restore();
    }

    fn draw_controls<C: Canvas>(&self, canvas: &mut C) {
        if self.controls_alpha <= 0.001 {
            return;
        }
        let Some(rects) = self.control_rects() else {
            return;
        };
        for (index, rect) in rects.iter().enumerate() {
            let hovered = self.hover_button == Some(index);
            let active = index == 0 && self.locked;
            let alpha = if hovered || active { 1.0 } else { 0.78 * 0.82 } * self.controls_alpha;
            let center_x = (rect.left + rect.right) / 2.0;
            let center_y = (rect.top + rect.bottom) / 2.0;
            let color = Color::new(1.0, 1.0, 1.0, alpha);
            let stroke = Stroke::round(ICON_STROKE);
            canvas.save();
            canvas.concat_transform(Transform {
                scale: ICON_SIZE / 24.0,
                dx: center_x - ICON_SIZE / 2.0,
                dy: center_y - ICON_SIZE / 2.0,
            });
            let (key, d) = match index {
                0 => {
                    canvas.stroke_rounded_rect(
                        Rect::new(3.0, 11.0, 21.0, 22.0),
                        2.0,
                        color,
                        stroke,
                    );
                    if self.locked {
                        ("subtitle-lock", ICON_LOCK_SHACKLE)
                    } else {
                        ("subtitle-unlock", ICON_UNLOCK_SHACKLE)
                    }
                }
                1 => ("subtitle-reset", ICON_ROTATE),
                _ => ("subtitle-close", ICON_CLOSE),
            };
            canvas.stroke_icon(&SvgIcon { key, d }, color, stroke);
            canvas.restore();
        }
    }
}

fn build_text_layout<T: TextEngine>(
    text: &T,
    value: &str,
    flow_width: f32,
    flow_height: f32,
    replace: bool,
    line_height: f32,
    config: &SubtitleConfig,
) -> Option<T::Layout> {
    text.layout(
        value,
        &TextStyle {
            family: FontFamily::Named(&config.font_family),
            size: config.font_size as f32,
            weight: 600,
            align: if replace {
                TextAlign::Leading
            } else {
                TextAlign::Center
            },
            paragraph: ParagraphAlign::Near,
            wrap: !replace,
            line_spacing: Some(UniformLineSpacing {
                height: line_height,
                baseline: line_height * BASELINE_RATIO,
            }),
        },
        if replace { 1e6 } else { flow_width },
        if replace {
            line_height * 4.0
        } else {
            flow_height * 100.0
        },
    )
}
fn layout_size<T: TextEngine>(text: &T, layout: &T::Layout) -> (f32, f32) {
    let metrics = text.metrics(layout);
    (metrics.width, metrics.height)
}

pub struct Subtitle<L> {
    view: SubtitleView<L>,
    bounds: PxRect,
    placed: bool,
    visible: bool,
    transition: Transition,
    drag_epoch: u64,
}
impl<L: Clone> Subtitle<L> {
    pub fn new(dpi: u32, _look: &Look) -> Self {
        let config = SubtitleConfig::default();
        Self {
            view: SubtitleView {
                layout_w: config.window_width as f32,
                layout_h: config.window_height as f32,
                anchor: config.anchor.clone(),
                offset_y: config.offset_y as f32,
                config,
                locked: false,
                tracks: [TextTrack::default(), TextTrack::default()],
                hovering_block: false,
                hover_button: None,
                controls_alpha: 0.0,
                controls_anim: None,
                dpi,
            },
            bounds: PxRect::default(),
            placed: false,
            visible: false,
            transition: Transition::new(false),
            drag_epoch: 0,
        }
    }
    fn apply_configured_placement(&mut self, area: PxRect) -> Ops<SubtitleAction> {
        let scale = if self.view.dpi == 0 {
            1.0
        } else {
            self.view.dpi as f32 / 96.0
        };
        let width = (self.view.layout_w * scale).round() as i32;
        let height = (self.view.layout_h * scale).round() as i32;
        let margin = (self.view.offset_y * scale).round() as i32;
        let (x, y) = window_position(
            area.x,
            area.y,
            area.width,
            area.height,
            width,
            height,
            &self.view.anchor,
            margin,
        );
        self.bounds = PxRect::new(x, y, width, height);
        self.placed = true;
        vec![WindowOp::SetBounds(self.bounds), WindowOp::Redraw]
    }
    fn finish_hide(&mut self) {
        self.visible = false;
        self.view.tracks[0].reset();
        self.view.tracks[1].reset();
        self.view.hovering_block = false;
        self.view.hover_button = None;
        self.view.controls_alpha = 0.0;
        self.view.controls_anim = None;
    }
    pub fn pointer_hit(&self) -> SubtitleHit {
        SubtitleHit {
            dpi: self.view.dpi,
            block: self.view.original_block_rect(),
            buttons: self.view.control_rects(),
            locked: self.view.locked,
            hovering_block: self.view.hovering_block,
            hover_button: self.view.hover_button.map(SubtitleButton::from_index),
            drag_epoch: self.drag_epoch,
        }
    }
}
impl<T: TextEngine> OverlayModel<T> for Subtitle<T::Layout> {
    type Command = Command;
    type Action = SubtitleAction;
    type Pointer = SubtitlePointer;
    fn apply(&mut self, command: Command, cx: &Cx<'_, T>) -> Ops<SubtitleAction> {
        match command {
            Command::SetConfig(config) => {
                let font_changed = self.view.config.font_family != config.font_family
                    || self.view.config.font_size != config.font_size;
                let mode_changed = self.view.config.display_mode != config.display_mode;
                let flow_changed = self.view.config.width != config.width
                    || self.view.config.line_count != config.line_count;
                self.view.config = *config;
                if font_changed || mode_changed || flow_changed {
                    self.view.reflow_tracks(cx.text, cx.now);
                }
                vec![WindowOp::Redraw]
            }
            Command::SetLayout {
                width,
                height,
                anchor,
                offset_y,
            } => {
                let width_changed = self.view.layout_w != width;
                self.view.layout_w = width;
                self.view.layout_h = height;
                self.view.anchor = anchor;
                self.view.offset_y = offset_y;
                if width_changed {
                    self.view.reflow_tracks(cx.text, cx.now);
                }
                self.apply_configured_placement(cx.area)
            }
            Command::SetText { text, fade } => {
                self.view.set_track_text(0, &text, fade, cx.text, cx.now);
                vec![WindowOp::Redraw]
            }
            Command::SetTranslation(text) => {
                self.view.set_track_text(1, &text, false, cx.text, cx.now);
                vec![WindowOp::Redraw]
            }
            Command::Show => {
                self.visible = true;
                self.transition.show();
                vec![WindowOp::Redraw, WindowOp::RaiseTopmost]
            }
            Command::Hide => {
                if self.transition.hide() {
                    vec![]
                } else {
                    self.finish_hide();
                    vec![WindowOp::Hide]
                }
            }
        }
    }
    fn input(&mut self, input: SubtitleInput, cx: &Cx<'_, T>) -> Ops<SubtitleAction> {
        match input {
            SubtitleInput::Hover { over_block, button } => {
                if over_block != self.view.hovering_block {
                    self.view.hovering_block = over_block;
                    let target = if over_block && self.view.tracks[0].has_text() {
                        1.0
                    } else {
                        0.0
                    };
                    self.view.animate_controls(target, cx.now);
                }
                self.view.hover_button = button.map(SubtitleButton::index);
            }
            SubtitleInput::Leave => {
                self.view.hovering_block = false;
                self.view.hover_button = None;
                self.view.animate_controls(0.0, cx.now);
            }
            SubtitleInput::Button(SubtitleButton::Lock) => self.view.locked = !self.view.locked,
            SubtitleInput::Button(SubtitleButton::Reset) => {
                let config = &self.view.config;
                self.view.layout_w = config.window_width as f32;
                self.view.layout_h = config.window_height as f32;
                self.view.anchor = config.anchor.clone();
                self.view.offset_y = config.offset_y as f32;
                // 重置仅恢复窗口几何，保留轨道缓存与位移目标。
                return self.apply_configured_placement(cx.area);
            }
            SubtitleInput::Button(SubtitleButton::Close) => {
                return vec![WindowOp::Action(SubtitleAction::Close)]
            }
        }
        vec![WindowOp::Redraw]
    }
    fn window_event(&mut self, event: WindowEvent, _cx: &Cx<'_, T>) -> Ops<SubtitleAction> {
        match event {
            WindowEvent::Moved { x, y } => {
                self.bounds.x = x;
                self.bounds.y = y;
            }
            WindowEvent::SystemDragEnded { bounds } => {
                self.bounds.x = bounds.x;
                self.bounds.y = bounds.y;
                self.drag_epoch = self.drag_epoch.wrapping_add(1);
            }
            WindowEvent::DpiChanged { dpi, suggested } if dpi != 0 && dpi != self.view.dpi => {
                self.view.dpi = dpi;
                let scale = dpi as f32 / 96.0;
                self.bounds = PxRect::new(
                    suggested.x,
                    suggested.y,
                    (self.view.layout_w * scale).round() as i32,
                    (self.view.layout_h * scale).round() as i32,
                );
                return vec![WindowOp::SetBounds(self.bounds), WindowOp::Redraw];
            }
            _ => {}
        }
        vec![]
    }
    fn set_look(&mut self, _look: &Look) -> Ops<SubtitleAction> {
        vec![WindowOp::Redraw]
    }
    fn on_frame(&mut self, _dt: f32, cx: &Cx<'_, T>) -> Ops<SubtitleAction> {
        self.view.tick(cx.now);
        let (_, finished) = self.transition.tick(cx.now);
        if finished && self.transition.is_fully_hidden() {
            self.finish_hide();
            vec![WindowOp::Hide, WindowOp::Redraw]
        } else {
            vec![WindowOp::Redraw]
        }
    }
    fn wants_frames(&self, _now: Instant) -> bool {
        self.visible
            && (self.transition.is_animating()
                || self.view.tracks[0].animating()
                || self.view.tracks[1].animating()
                || self.view.controls_anim.is_some())
    }
    fn prepare_frame(&mut self, cx: &Cx<'_, T>) -> Option<FrameSpec> {
        if !self.placed {
            self.apply_configured_placement(cx.area);
        }
        if !self.visible {
            return None;
        }
        if cx.dpi != 0 {
            self.view.dpi = cx.dpi;
        }
        self.view.prepare_layouts(cx.text);
        let visual = self.transition.visual();
        let scale = if self.view.dpi == 0 {
            1.0
        } else {
            self.view.dpi as f32 / 96.0
        };
        let slide = ((1.0 - visual) * TRANSITION_SLIDE_PX * scale).round() as i32;
        Some(FrameSpec {
            bounds: PxRect::new(
                self.bounds.x,
                self.bounds.y + slide,
                self.bounds.width,
                self.bounds.height,
            ),
            dpi: self.view.dpi,
            alpha: visual,
        })
    }
    fn draw<C: Canvas<Text = T>>(&self, canvas: &mut C, now: Instant) {
        self.view.draw(canvas, now);
    }
    fn hit(&self) -> SubtitleHit {
        self.pointer_hit()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{Draw, FakeLayout, FakeText, RecordingCanvas};
    use std::rc::Rc;
    use std::time::Duration;
    type Model = Subtitle<Rc<FakeLayout>>;
    fn cx(now: Instant, text: &FakeText) -> Cx<'_, FakeText> {
        Cx {
            now,
            dpi: 96,
            area: PxRect::new(-1920, 0, 1920, 1080),
            text,
        }
    }
    fn set_text(model: &mut Model, text: &str, fade: bool, cx: &Cx<'_, FakeText>) {
        model.apply(
            Command::SetText {
                text: text.into(),
                fade,
            },
            cx,
        );
    }
    fn config(model: &mut Model, value: SubtitleConfig, cx: &Cx<'_, FakeText>) {
        model.apply(Command::SetConfig(Box::new(value)), cx);
    }
    #[test]
    fn window_commands_reflow_both_tracks_without_mouse_input() {
        let text = FakeText;
        let now = Instant::now();
        let cx = cx(now, &text);
        let mut model = Model::new(96, &Look::WINDOWS_LEGACY);
        model.apply(
            Command::SetLayout {
                width: 444.0,
                height: 136.0,
                anchor: "center".into(),
                offset_y: 0.0,
            },
            &cx,
        );
        let mut prefs = SubtitleConfig {
            display_mode: "replace".into(),
            width: 444.0,
            window_width: 444.0,
            window_height: 136.0,
            line_count: 1,
            translation_enabled: true,
            motion_enabled: false,
            fade_enabled: false,
            ..SubtitleConfig::default()
        };
        config(&mut model, prefs.clone(), &cx);
        set_text(&mut model, "原生字幕 Hello", false, &cx);
        model.apply(Command::SetTranslation("Bilingual translation".into()), &cx);
        assert_eq!(
            model
                .view
                .blocks()
                .iter()
                .map(|(i, _)| *i)
                .collect::<Vec<_>>(),
            [1, 0]
        );
        set_text(
            &mut model,
            &"连续追加长句显示最新内容".repeat(12),
            false,
            &cx,
        );
        assert!(model.view.tracks[0].offset < 0.0);
        prefs.display_mode = "scroll".into();
        prefs.line_count = 2;
        prefs.translation_order = "sourceFirst".into();
        model.apply(
            Command::SetLayout {
                width: 444.0,
                height: 214.0,
                anchor: "center".into(),
                offset_y: 0.0,
            },
            &cx,
        );
        config(&mut model, prefs.clone(), &cx);
        assert_eq!(
            model
                .view
                .blocks()
                .iter()
                .map(|(i, _)| *i)
                .collect::<Vec<_>>(),
            [0, 1]
        );
        let before = model.view.tracks[0].content_size.1;
        prefs.width = 260.0;
        config(&mut model, prefs.clone(), &cx);
        assert!(model.view.tracks[0].content_size.1 > before);
        let offset = model.view.tracks[0].offset;
        prefs.line_count = 1;
        config(&mut model, prefs.clone(), &cx);
        assert_eq!(model.view.tracks[0].offset - offset, 39.0);
        model.apply(
            Command::SetLayout {
                width: 200.0,
                height: 214.0,
                anchor: "center".into(),
                offset_y: 0.0,
            },
            &cx,
        );
        assert_eq!(model.view.flow_width(), 154.0);
        assert_eq!(
            model.view.tracks[0].layout.as_ref().unwrap().metrics.width,
            154.0
        );
        model.apply(Command::SetTranslation(String::new()), &cx);
        assert_eq!(model.view.blocks().len(), 1);
        prefs.fade_enabled = true;
        config(&mut model, prefs, &cx);
        set_text(&mut model, "新字幕", false, &cx);
        assert!(model.view.tracks[0].fresh_started.is_some());
        let later = Cx {
            now: now + Duration::from_secs(1),
            ..cx
        };
        model.on_frame(0.016, &later);
        assert!(model.view.tracks[0].fresh_started.is_none());
        set_text(&mut model, "整段换新", true, &later);
        let later = Cx {
            now: later.now + Duration::from_secs(1),
            ..later
        };
        model.on_frame(0.016, &later);
        assert!(model.view.tracks[0].swap_started.is_none());
    }
    #[test]
    fn replace_text_stays_in_view_for_short_and_overflowing_lines() {
        let text = FakeText;
        let cx = cx(Instant::now(), &text);
        let mut model = Model::new(96, &Look::WINDOWS_LEGACY);
        config(
            &mut model,
            SubtitleConfig {
                display_mode: "replace".into(),
                line_count: 1,
                motion_enabled: false,
                ..SubtitleConfig::default()
            },
            &cx,
        );
        for value in [
            "原生字幕 Hello".into(),
            "长句持续追加显示最新内容".repeat(12),
        ] {
            set_text(&mut model, &value, false, &cx);
            let track = &model.view.tracks[0];
            let metrics = text.metrics(track.layout.as_ref().unwrap());
            let left = metrics.left + track.offset;
            let right = left + metrics.width;
            let width = model.view.flow_width();
            assert!(left < width && right > 0.0);
            if metrics.width > width {
                assert!((right - width).abs() < 0.1);
            } else {
                assert!((left - (width - right)).abs() < 0.1);
            }
            assert_eq!(metrics.line_count, 1);
            assert_eq!(metrics.height, 39.0);
        }
    }
    #[test]
    fn scroll_text_keeps_centered_lines_and_reveals_the_latest_line() {
        let text = FakeText;
        let cx = cx(Instant::now(), &text);
        let mut model = Model::new(96, &Look::WINDOWS_LEGACY);
        config(
            &mut model,
            SubtitleConfig {
                motion_enabled: false,
                ..SubtitleConfig::default()
            },
            &cx,
        );
        set_text(&mut model, &"第一行第二行最新一行".repeat(20), false, &cx);
        let track = &model.view.tracks[0];
        let metrics = text.metrics(track.layout.as_ref().unwrap());
        assert_eq!(
            metrics.left,
            (model.view.flow_width() - metrics.width) / 2.0
        );
        assert!(metrics.line_count > 2);
        assert_eq!(metrics.height - track.offset, model.view.flow_height());
    }
    #[test]
    fn fresh_uses_utf16_and_swap_uses_blur_until_deadline() {
        let text = FakeText;
        let now = Instant::now();
        let cx = cx(now, &text);
        let mut model = Model::new(96, &Look::WINDOWS_LEGACY);
        model.apply(Command::Show, &cx);
        set_text(&mut model, "😀旧", false, &cx);
        set_text(&mut model, "😀旧新", false, &cx);
        let later = Cx {
            now: now + Duration::from_millis(90),
            ..cx
        };
        model.prepare_frame(&later);
        let cached = model.view.tracks[0].layout.clone().unwrap();
        let mut canvas = RecordingCanvas::new(96);
        model.draw(&mut canvas, later.now);
        assert!(canvas.calls.iter().any(|draw| matches!(draw, Draw::Text(_, _, _, Some(tint)) if tint.utf16_start == 3 && tint.utf16_len == 1 && tint.color.a > 0.0 && tint.color.a < 1.0)));
        model.prepare_frame(&later);
        assert!(Rc::ptr_eq(
            &cached,
            model.view.tracks[0].layout.as_ref().unwrap()
        ));
        set_text(&mut model, "换新", true, &later);
        canvas.calls.clear();
        model.draw(&mut canvas, later.now + Duration::from_millis(100));
        assert!(canvas
            .calls
            .iter()
            .any(|draw| matches!(draw, Draw::Blurred(_, _, _, _, sigma) if *sigma > 0.0)));
        let end = Cx {
            now: later.now + Duration::from_millis(320),
            ..later
        };
        model.on_frame(0.016, &end);
        assert!(model.view.tracks[0].swap_started.is_none());
    }
    #[test]
    fn controls_buttons_reset_and_hide_keep_legacy_semantics() {
        let text = FakeText;
        let now = Instant::now();
        let cx = cx(now, &text);
        let mut model = Model::new(96, &Look::WINDOWS_LEGACY);
        assert_eq!(model.apply(Command::Hide, &cx), [WindowOp::Hide]);
        assert_eq!(
            model.apply(Command::Show, &cx),
            [WindowOp::Redraw, WindowOp::RaiseTopmost]
        );
        set_text(&mut model, "字幕", false, &cx);
        model.input(
            SubtitleInput::Hover {
                over_block: true,
                button: None,
            },
            &cx,
        );
        let mid = Cx {
            now: now + Duration::from_millis(60),
            ..cx
        };
        model.on_frame(0.016, &mid);
        assert!(model.view.controls_alpha > 0.0 && model.view.controls_alpha < 1.0);
        model.input(SubtitleInput::Leave, &mid);
        let end = Cx {
            now: mid.now + Duration::from_millis(120),
            ..mid
        };
        model.on_frame(0.016, &end);
        assert_eq!(model.view.controls_alpha, 0.0);
        model.input(SubtitleInput::Button(SubtitleButton::Lock), &cx);
        assert!(model.pointer_hit().locked);
        assert_eq!(
            model.input(SubtitleInput::Button(SubtitleButton::Close), &cx),
            [WindowOp::Action(SubtitleAction::Close)]
        );
        model.apply(
            Command::SetLayout {
                width: 200.0,
                height: 100.0,
                anchor: "top".into(),
                offset_y: 0.0,
            },
            &cx,
        );
        let layout = model.view.tracks[0].layout.clone().unwrap();
        let ops = model.input(SubtitleInput::Button(SubtitleButton::Reset), &cx);
        assert_eq!(
            ops,
            [
                WindowOp::SetBounds(PxRect::new(-1400, 910, 880, 134)),
                WindowOp::Redraw
            ]
        );
        assert!(
            Rc::ptr_eq(&layout, model.view.tracks[0].layout.as_ref().unwrap()),
            "重置不重排轨道"
        );
        // WM_MOVE 的位置包含过渡位移时也照常写回，保留 Windows 的现有定位语义。
        model.window_event(WindowEvent::Moved { x: -100, y: 200 }, &cx);
        assert_eq!(model.bounds.x, -100);
        assert_eq!(model.bounds.y, 200);
        model.window_event(
            WindowEvent::DpiChanged {
                dpi: 144,
                suggested: PxRect::new(-50, 60, 0, 0),
            },
            &cx,
        );
        assert_eq!(model.bounds, PxRect::new(-50, 60, 1320, 201));
        model.apply(Command::Hide, &end);
        model.on_frame(0.016, &end);
        let hidden = Cx {
            now: end.now + Duration::from_secs(1),
            ..end
        };
        assert_eq!(
            model.on_frame(0.016, &hidden),
            [WindowOp::Hide, WindowOp::Redraw]
        );
        assert!(!model.visible);
        assert!(!model.view.tracks[0].has_text());
        assert!(!<Model as OverlayModel<FakeText>>::wants_frames(
            &model, hidden.now
        ));
    }
}
