//! 会议录制条：录制中标记、已录时长、麦克风与系统声音的音量、停止按钮。可拖动。
//!
//! 不开帧时钟：计时与音量靠调用方定时投递 `Levels` 触发重绘，只有出入场过渡期间才逐帧推进。
use crate::anim::Transition;
use crate::canvas::{Canvas, SvgIcon, TextEngine};
use crate::geom::{Point, PxRect, Rect, Shape, Stroke, Transform};
use crate::indicator::wave_scale;
use crate::look::{Look, Profile};
use crate::ui::layout::{inset, stack, Align, Axis, Item};
use crate::ui::paint::{self, LevelBars};
use crate::ui::{HitKind, HitMap, UiInput, UiPointer};
use crate::window::*;
use std::time::Instant;

const WIDTH: f32 = 208.0;
const HEIGHT: f32 = 36.0;
const RADIUS: f32 = HEIGHT / 2.0;
const PAD_X: f32 = 10.0;
const GAP: f32 = 8.0;
/// 没有记住的位置时，条顶离参考区顶边的距离。
const DEFAULT_TOP: f32 = 24.0;

const DOT: f32 = 8.0;
const TIMER_SIZE: f32 = 13.0;
const ICON: f32 = 14.0;
const ICON_GAP: f32 = 3.0;
const METER: f32 = 16.0;
const BUTTON: f32 = 24.0;
const BUTTON_RADIUS: f32 = 6.0;
const STOP_MARK: f32 = 10.0;
const STOP_MARK_RADIUS: f32 = 2.5;

const METER_BARS: usize = 4;
const METER_STYLE: LevelBars = LevelBars {
    bar_ratio: 0.125,
    gap_ratio: 0.125,
    heights: &[1.0; METER_BARS],
    min_scale: 0.12,
};

const ERR: (f32, f32, f32) = (229.0 / 255.0, 114.0 / 255.0, 108.0 / 255.0); // --color-err
const WARN: (f32, f32, f32) = (217.0 / 255.0, 164.0 / 255.0, 65.0 / 255.0); // --color-warn

// Lucide 的 mic 与 volume-1，24×24 视框。
const MIC_ICON: SvgIcon = SvgIcon {
    key: "recorder-mic",
    d: "M12 2a3 3 0 0 0-3 3v7a3 3 0 0 0 6 0V5a3 3 0 0 0-3-3M19 10v2a7 7 0 0 1-14 0v-2M12 19v3",
};
const SYSTEM_ICON: SvgIcon = SvgIcon {
    key: "recorder-system",
    d: "M11 5L6 9H2v6h4l5 4V5M15.54 8.46a5 5 0 0 1 0 7.07",
};

const BODY_ID: u16 = 0;
const STOP_ID: u16 = 1;

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    /// 显示并从零计时。position 为记住的左上角（DesktopPx）；None 时放在参考区顶部居中。
    Show { position: Option<(i32, i32)> },
    /// 两路最近一小段的音量（RMS，0..1）。每次投递都会刷新计时。
    Levels { mic: f32, system: f32 },
    /// 哪一路中途断开了。
    SetLost { mic: bool, system: bool },
    Hide,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecorderAction {
    /// 用户点了停止。
    Stop,
    /// 用户把条拖到了新位置（左上角，DesktopPx）。
    Moved { x: i32, y: i32 },
}

struct Parts {
    panel: Rect,
    dot: Rect,
    timer: Rect,
    /// 麦克风、系统声音两路：图标与音量柱。
    tracks: [(Rect, Rect); 2],
    stop: Rect,
}

fn parts() -> Parts {
    let panel = Rect::new(0.0, 0.0, WIDTH, HEIGHT);
    let track = ICON + ICON_GAP + METER;
    let row = stack(
        Axis::Row,
        inset(panel, PAD_X, 0.0),
        GAP,
        Align::Center,
        &[
            Item::fixed(DOT, DOT),
            Item::flex(HEIGHT),
            Item::fixed(track, METER),
            Item::fixed(track, METER),
            Item::fixed(BUTTON, BUTTON),
        ],
    );
    let split = |rect: Rect| {
        let pair = stack(
            Axis::Row,
            rect,
            ICON_GAP,
            Align::Center,
            &[Item::fixed(ICON, ICON), Item::fixed(METER, METER)],
        );
        (pair[0], pair[1])
    };
    Parts {
        panel,
        dot: row[0],
        timer: row[1],
        tracks: [split(row[2]), split(row[3])],
        stop: row[4],
    }
}

pub struct Recorder {
    look: Look,
    dpi: u32,
    origin: (i32, i32),
    visible: bool,
    transition: Transition,
    started: Option<Instant>,
    /// 每路最近几次的音量，最新的在末尾。
    levels: [[f32; METER_BARS]; 2],
    lost: [bool; 2],
    /// 已点停止、等调用方收尾：按钮不再响应。
    stopping: bool,
    hover: bool,
    pressed: bool,
}

impl Recorder {
    pub fn new(dpi: u32, look: &Look) -> Self {
        Self {
            look: *look,
            dpi: dpi.max(1),
            origin: (0, 0),
            visible: false,
            transition: Transition::new(false),
            started: None,
            levels: [[0.0; METER_BARS]; 2],
            lost: [false; 2],
            stopping: false,
            hover: false,
            pressed: false,
        }
    }
    fn bounds(&self) -> PxRect {
        let scale = self.dpi as f32 / 96.0;
        PxRect::new(
            self.origin.0,
            self.origin.1,
            (WIDTH * scale).round() as i32,
            (HEIGHT * scale).round() as i32,
        )
    }
    fn elapsed_seconds(&self, now: Instant) -> u64 {
        self.started
            .map_or(0, |started| now.saturating_duration_since(started).as_secs())
    }
    fn color(&self, rgb: (f32, f32, f32), alpha: f32) -> crate::geom::Color {
        self.look.color(rgb.0, rgb.1, rgb.2, alpha)
    }
    fn draw_icon<C: Canvas>(&self, canvas: &mut C, icon: &SvgIcon, rect: Rect, lost: bool) {
        let color = if lost {
            self.color(WARN, 1.0)
        } else {
            paint::fg(&self.look, 0.72)
        };
        canvas.save();
        canvas.concat_transform(Transform {
            scale: ICON / 24.0,
            dx: rect.left,
            dy: rect.top,
        });
        canvas.stroke_icon(icon, color, Stroke::round(2.0));
        canvas.restore();
    }
}

impl<T: TextEngine> OverlayModel<T> for Recorder {
    type Command = Command;
    type Action = RecorderAction;
    type Pointer = UiPointer<RecorderAction>;

    fn apply(&mut self, command: Command, cx: &Cx<'_, T>) -> Ops<RecorderAction> {
        match command {
            Command::Show { position } => {
                if cx.dpi != 0 {
                    self.dpi = cx.dpi;
                }
                let size = self.bounds();
                self.origin = position.unwrap_or_else(|| {
                    let top = (DEFAULT_TOP * self.dpi as f32 / 96.0).round() as i32;
                    (
                        cx.area.x + (cx.area.width - size.width) / 2,
                        cx.area.y + top,
                    )
                });
                self.visible = true;
                self.transition.show();
                self.started = Some(cx.now);
                self.levels = [[0.0; METER_BARS]; 2];
                self.lost = [false; 2];
                self.stopping = false;
                self.hover = false;
                self.pressed = false;
                vec![
                    WindowOp::SetBounds(self.bounds()),
                    WindowOp::SetInteractive(true),
                    WindowOp::Redraw,
                    WindowOp::RaiseTopmost,
                ]
            }
            Command::Levels { mic, system } => {
                for (history, level) in self.levels.iter_mut().zip([mic, system]) {
                    history.rotate_left(1);
                    history[METER_BARS - 1] = wave_scale(level);
                }
                vec![WindowOp::Redraw]
            }
            Command::SetLost { mic, system } => {
                self.lost = [mic, system];
                vec![WindowOp::Redraw]
            }
            Command::Hide => {
                if self.transition.hide() {
                    vec![WindowOp::SetInteractive(false)]
                } else {
                    self.visible = false;
                    vec![WindowOp::Hide]
                }
            }
        }
    }
    fn input(&mut self, input: UiInput, _: &Cx<'_, T>) -> Ops<RecorderAction> {
        match input {
            UiInput::Hover(id) => self.hover = id == Some(STOP_ID),
            UiInput::Press(id) => self.pressed = id == Some(STOP_ID),
            UiInput::Click(_) => {
                self.stopping = true;
                self.hover = false;
                return vec![
                    WindowOp::Redraw,
                    WindowOp::Action(RecorderAction::Stop),
                ];
            }
        }
        vec![WindowOp::Redraw]
    }
    fn window_event(&mut self, event: WindowEvent, _: &Cx<'_, T>) -> Ops<RecorderAction> {
        match event {
            WindowEvent::Moved { x, y } => {
                self.origin = (x, y);
                Vec::new()
            }
            WindowEvent::SystemDragEnded { bounds } => {
                self.origin = (bounds.x, bounds.y);
                vec![WindowOp::Action(RecorderAction::Moved {
                    x: bounds.x,
                    y: bounds.y,
                })]
            }
            WindowEvent::DpiChanged { dpi, suggested } => {
                if dpi == 0 || dpi == self.dpi {
                    return Vec::new();
                }
                self.dpi = dpi;
                self.origin = (suggested.x, suggested.y);
                vec![WindowOp::SetBounds(self.bounds()), WindowOp::Redraw]
            }
            WindowEvent::AreaChanged => Vec::new(),
        }
    }
    fn set_look(&mut self, look: &Look) -> Ops<RecorderAction> {
        self.look = *look;
        vec![WindowOp::Redraw]
    }
    fn on_frame(&mut self, _: f32, cx: &Cx<'_, T>) -> Ops<RecorderAction> {
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
    fn prepare_frame(&mut self, _: &Cx<'_, T>) -> Option<FrameSpec> {
        self.visible.then(|| FrameSpec {
            bounds: self.bounds(),
            dpi: self.dpi,
            alpha: self.transition.visual(),
        })
    }
    fn draw<C: Canvas<Text = T>>(&self, canvas: &mut C, now: Instant) {
        let parts = parts();
        paint::panel(canvas, &self.look, parts.panel, RADIUS, 1.0, 0.0);
        let any_lost = self.lost.contains(&true);
        canvas.fill_circle(
            Point::new(
                (parts.dot.left + parts.dot.right) / 2.0,
                (parts.dot.top + parts.dot.bottom) / 2.0,
            ),
            DOT / 2.0,
            self.color(if any_lost { WARN } else { ERR }, 1.0),
        );
        paint::label(
            canvas,
            &paint::format_elapsed(self.elapsed_seconds(now)),
            &paint::label_style(TIMER_SIZE, 600),
            parts.timer,
            paint::fg(&self.look, 1.0),
        );
        for (index, icon) in [&MIC_ICON, &SYSTEM_ICON].into_iter().enumerate() {
            let (icon_rect, meter) = parts.tracks[index];
            let lost = self.lost[index];
            self.draw_icon(canvas, icon, icon_rect, lost);
            // 断开的一路只留最矮的柱，并换成警告色。
            let levels = if lost {
                [0.0; METER_BARS]
            } else {
                self.levels[index]
            };
            let color = if lost {
                self.color(WARN, 0.9)
            } else {
                paint::fg(&self.look, 0.9)
            };
            METER_STYLE.draw(
                canvas,
                self.dpi,
                Point::new(meter.left, meter.top),
                METER,
                &levels,
                color,
            );
        }
        let active = !self.stopping;
        paint::button_backdrop(
            canvas,
            &self.look,
            parts.stop,
            BUTTON_RADIUS,
            active && self.hover,
            active && self.pressed,
        );
        let center = Point::new(
            (parts.stop.left + parts.stop.right) / 2.0,
            (parts.stop.top + parts.stop.bottom) / 2.0,
        );
        canvas.fill_rounded_rect(
            Rect::new(
                center.x - STOP_MARK / 2.0,
                center.y - STOP_MARK / 2.0,
                center.x + STOP_MARK / 2.0,
                center.y + STOP_MARK / 2.0,
            ),
            STOP_MARK_RADIUS,
            self.color(ERR, if active { 1.0 } else { 0.4 }),
        );
    }
    fn hit(&self) -> HitMap {
        let mut map = HitMap::new(self.dpi);
        if self.visible {
            let parts = parts();
            map.push(BODY_ID, parts.panel, HitKind::Drag);
            if !self.stopping {
                map.push(STOP_ID, parts.stop, HitKind::Button);
            }
        }
        map.set_hover(self.hover.then_some(STOP_ID));
        map
    }
    fn glass(&self) -> Vec<Shape> {
        if self.look.profile != Profile::Css || self.look.glass.is_none() || !self.visible {
            return Vec::new();
        }
        vec![Shape::RoundedRect {
            rect: Rect::new(0.0, 0.0, WIDTH, HEIGHT),
            radius: RADIUS,
        }]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{Draw, FakeText, RecordingCanvas};
    use std::time::Duration;

    fn cx(now: Instant, dpi: u32, text: &FakeText) -> Cx<'_, FakeText> {
        Cx {
            now,
            dpi,
            area: PxRect::new(100, 50, 2000, 1000),
            text,
        }
    }
    fn shown(now: Instant, dpi: u32, position: Option<(i32, i32)>) -> Recorder {
        let mut model = Recorder::new(96, &Look::MACOS_DEFAULT);
        model.apply(
            Command::Show { position },
            &cx(now, dpi, &FakeText),
        );
        model
    }
    fn texts(model: &Recorder, now: Instant) -> Vec<String> {
        let mut canvas = RecordingCanvas::new(96);
        model.draw(&mut canvas, now);
        canvas
            .calls
            .into_iter()
            .filter_map(|call| match call {
                Draw::Text(text, ..) => Some(text),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn show_centers_at_the_top_by_default_and_honours_a_remembered_position() {
        let now = Instant::now();
        let mut model = Recorder::new(96, &Look::MACOS_DEFAULT);
        let ops = model.apply(
            Command::Show { position: None },
            &cx(now, 192, &FakeText),
        );
        let bounds = PxRect::new(100 + (2000 - 416) / 2, 50 + 48, 416, 72);
        assert_eq!(
            ops,
            vec![
                WindowOp::SetBounds(bounds),
                WindowOp::SetInteractive(true),
                WindowOp::Redraw,
                WindowOp::RaiseTopmost,
            ]
        );
        let mut model = shown(now, 96, Some((-300, 40)));
        assert_eq!(
            model.prepare_frame(&cx(now, 96, &FakeText)).unwrap().bounds,
            PxRect::new(-300, 40, 208, 36)
        );
    }

    #[test]
    fn timer_counts_from_show_and_switches_to_hours() {
        let now = Instant::now();
        let model = shown(now, 96, None);
        assert_eq!(texts(&model, now + Duration::from_secs(75)), ["01:15"]);
        assert_eq!(texts(&model, now + Duration::from_secs(3605)), ["1:00:05"]);
    }

    #[test]
    fn stop_click_reports_once_and_then_the_button_stops_responding() {
        let now = Instant::now();
        let mut model = shown(now, 96, None);
        let hit = <Recorder as OverlayModel<FakeText>>::hit(&model);
        let stop = parts().stop;
        let on_stop = ((stop.left + 4.0) as i32, (stop.top + 4.0) as i32);
        assert_eq!(hit.at(on_stop).map(|r| r.kind), Some(HitKind::Button));
        assert_eq!(hit.at((30, 18)).map(|r| r.kind), Some(HitKind::Drag));
        let ops = model.input(UiInput::Click(STOP_ID), &cx(now, 96, &FakeText));
        assert_eq!(
            ops,
            vec![
                WindowOp::Redraw,
                WindowOp::Action(RecorderAction::Stop)
            ]
        );
        let hit = <Recorder as OverlayModel<FakeText>>::hit(&model);
        assert_eq!(hit.at(on_stop).map(|r| r.kind), Some(HitKind::Drag));
    }

    #[test]
    fn drag_end_reports_the_new_position_and_dpi_change_rescales_in_place() {
        let now = Instant::now();
        let mut model = shown(now, 96, Some((10, 20)));
        let context = cx(now, 96, &FakeText);
        assert_eq!(
            model.window_event(
                WindowEvent::SystemDragEnded {
                    bounds: PxRect::new(700, 80, 208, 36)
                },
                &context
            ),
            vec![WindowOp::Action(RecorderAction::Moved { x: 700, y: 80 })]
        );
        assert_eq!(
            model.window_event(
                WindowEvent::DpiChanged {
                    dpi: 144,
                    suggested: PxRect::new(900, 90, 1, 1)
                },
                &context
            ),
            vec![
                WindowOp::SetBounds(PxRect::new(900, 90, 312, 54)),
                WindowOp::Redraw
            ]
        );
    }

    #[test]
    fn a_lost_track_turns_its_icon_and_meter_to_the_warning_colour() {
        let now = Instant::now();
        let mut model = shown(now, 96, None);
        let context = cx(now, 96, &FakeText);
        model.apply(Command::Levels { mic: 0.2, system: 0.2 }, &context);
        model.apply(
            Command::SetLost {
                mic: false,
                system: true,
            },
            &context,
        );
        let mut canvas = RecordingCanvas::new(96);
        model.draw(&mut canvas, now);
        let warn = Look::MACOS_DEFAULT.color(WARN.0, WARN.1, WARN.2, 1.0);
        let icons: Vec<_> = canvas
            .calls
            .iter()
            .filter_map(|call| match call {
                Draw::Icon(key, color, _) => Some((*key, *color)),
                _ => None,
            })
            .collect();
        assert_eq!(icons.len(), 2);
        assert_ne!(icons[0].1, warn);
        assert_eq!(icons[1], ("recorder-system", warn));
        // 录制标记也换成警告色。
        assert!(canvas
            .calls
            .iter()
            .any(|call| matches!(call, Draw::FillCircle(_, _, color) if *color == warn)));
    }

    #[test]
    fn hide_fades_out_then_hides_and_frames_run_only_during_the_transition() {
        let now = Instant::now();
        let mut model = shown(now, 96, None);
        let wants = |model: &Recorder, at| <Recorder as OverlayModel<FakeText>>::wants_frames(model, at);
        model.on_frame(0.016, &cx(now, 96, &FakeText));
        let settled = now + Duration::from_millis(400);
        model.on_frame(0.4, &cx(settled, 96, &FakeText));
        assert!(!wants(&model, settled));
        assert_eq!(
            model.apply(Command::Hide, &cx(settled, 96, &FakeText)),
            vec![WindowOp::SetInteractive(false)]
        );
        assert!(wants(&model, settled));
        model.on_frame(0.016, &cx(settled, 96, &FakeText));
        let gone = settled + Duration::from_millis(400);
        let ops = model.on_frame(0.4, &cx(gone, 96, &FakeText));
        assert_eq!(ops, vec![WindowOp::Hide, WindowOp::Redraw]);
        assert!(model.prepare_frame(&cx(gone, 96, &FakeText)).is_none());
        assert!(!wants(&model, gone));
    }

    #[test]
    fn icon_paths_parse() {
        for icon in [&MIC_ICON, &SYSTEM_ICON] {
            assert!(!crate::svg::parse_svg_path(icon.d).unwrap().is_empty(), "{}", icon.key);
        }
    }
}
