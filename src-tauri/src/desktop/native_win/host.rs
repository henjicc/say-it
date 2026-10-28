//! Win32 消息先生成有序副作用，再释放状态借用执行；系统拖拽可进入嵌套消息循环。
use super::d2d::{D2dCanvas, D2dResources, DWriteText};
use super::{
    create_d2d_factory, overlay_now, system_dpi, window_dpi, LayeredSurface, OverlayThread,
};
use sayit_overlay_core::geom::PxRect;
use sayit_overlay_core::window::*;
use std::any::{Any, TypeId};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{
    GetLastError, ERROR_CLASS_ALREADY_EXISTS, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM,
};
use windows::Win32::Graphics::Direct2D::ID2D1Factory;
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTOPRIMARY,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    ReleaseCapture, SetCapture, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetCursorPos, GetWindowLongPtrW, GetWindowRect, KillTimer,
    LoadCursorW, RegisterClassW, SendMessageW, SetCursor, SetTimer, SetWindowLongPtrW,
    SetWindowPos, ShowWindow, GWLP_USERDATA, GWL_EXSTYLE, HTCAPTION, HTCLIENT, HWND_TOPMOST,
    IDC_ARROW, IDC_SIZEALL, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
    SW_HIDE, WINDOW_EX_STYLE, WM_CAPTURECHANGED, WM_DESTROY, WM_DPICHANGED, WM_LBUTTONDOWN,
    WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOVE, WM_NCLBUTTONDOWN, WM_RBUTTONUP, WM_SETCURSOR, WM_TIMER,
    WNDCLASSW, WS_EX_TRANSPARENT, WS_POPUP,
};

pub(crate) const FRAME_TIMER: usize = 1;
const FRAME_MS: u32 = 16;
// windows crate 将此常量放在未启用的 UI_Controls 特性中，值来自 winuser.h。
const WM_MOUSELEAVE_MSG: u32 = 0x02A3;

pub(crate) trait WinWindow: 'static {
    type Model: OverlayModel<DWriteText>;
    const CLASS_NAME: PCWSTR;
    const EX_STYLE: WINDOW_EX_STYLE;
    const THREAD_NAME: &'static str = "sayit-native-overlay";
    const TITLE: PCWSTR = w!("说吧！");
    fn create_model(dpi: u32) -> Self::Model;
    fn area(dpi: u32) -> PxRect;
    fn after_ops(_model: &Self::Model) {}
    fn dispatch(action: <Self::Model as OverlayModel<DWriteText>>::Action);
}
type Command<W> = <<W as WinWindow>::Model as OverlayModel<DWriteText>>::Command;
type Action<W> = <<W as WinWindow>::Model as OverlayModel<DWriteText>>::Action;
type Pointer<W> = <<W as WinWindow>::Model as OverlayModel<DWriteText>>::Pointer;

struct Host<W: WinWindow> {
    model: W::Model,
    pointer: Pointer<W>,
    text: DWriteText,
    d2d: ID2D1Factory,
    resources: D2dResources,
    surface: LayeredSurface,
    clock: FrameClock,
}

/// 仅用于不调用窗口系统的闭包；返回值必须拥有全部数据，不能把借用带到副作用中。
fn with_state<W: WinWindow, R>(hwnd: HWND, f: impl FnOnce(&mut Host<W>) -> R) -> Option<R> {
    let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const RefCell<Host<W>>;
    if ptr.is_null() {
        None
    } else {
        Some(f(&mut unsafe { &*ptr }.borrow_mut()))
    }
}

pub(crate) fn post<W: WinWindow>(command: Command<W>) {
    static THREADS: OnceLock<Mutex<HashMap<TypeId, Box<dyn Any + Send + Sync>>>> = OnceLock::new();
    let mut threads = THREADS
        .get_or_init(Default::default)
        .lock()
        .expect("overlay thread registry poisoned");
    let entry = threads.entry(TypeId::of::<W>()).or_insert_with(|| {
        let thread = OverlayThread::start(W::THREAD_NAME, create_window::<W>, apply::<W>)
            .map_err(|error| eprintln!("[{}] {error}", W::THREAD_NAME))
            .ok();
        Box::new(thread)
    });
    if let Some(thread) = entry
        .downcast_ref::<Option<OverlayThread<Command<W>>>>()
        .and_then(Option::as_ref)
    {
        thread.post(command);
    }
}

fn update<W: WinWindow>(
    hwnd: HWND,
    now: Instant,
    f: impl FnOnce(&mut W::Model, &Cx<'_, DWriteText>) -> Ops<Action<W>>,
) {
    let dpi = window_dpi(hwnd);
    let area = W::area(dpi);
    let ops = with_state::<W, _>(hwnd, |state| {
        f(
            &mut state.model,
            &Cx {
                now,
                dpi,
                area,
                text: &state.text,
            },
        )
    })
    .unwrap_or_default();
    execute::<W>(hwnd, ops, now);
    finish::<W>(hwnd, now);
}
pub(crate) fn apply<W: WinWindow>(hwnd: HWND, command: Command<W>) {
    update::<W>(hwnd, overlay_now(), |model, cx| model.apply(command, cx));
}
fn window_event<W: WinWindow>(hwnd: HWND, event: WindowEvent, now: Instant) {
    update::<W>(hwnd, now, |model, cx| model.window_event(event, cx));
}
fn sync_timer<W: WinWindow>(hwnd: HWND, now: Instant) {
    let change = with_state::<W, _>(hwnd, |state| {
        state.clock.sync(state.model.wants_frames(now))
    })
    .flatten();
    unsafe {
        match change {
            Some(true) => {
                SetTimer(hwnd, FRAME_TIMER, FRAME_MS, None);
            }
            Some(false) => {
                let _ = KillTimer(hwnd, FRAME_TIMER);
            }
            None => {}
        }
    }
}
fn finish<W: WinWindow>(hwnd: HWND, now: Instant) {
    sync_timer::<W>(hwnd, now);
    with_state::<W, _>(hwnd, |state| W::after_ops(&state.model));
}
fn redraw<W: WinWindow>(hwnd: HWND, now: Instant) {
    let dpi = window_dpi(hwnd);
    let area = W::area(dpi);
    let presentation = with_state::<W, _>(hwnd, |state| {
        let cx = Cx {
            now,
            dpi,
            area,
            text: &state.text,
        };
        let frame = state.model.prepare_frame(&cx)?;
        state.surface.draw_frame(
            &state.d2d,
            frame.dpi,
            frame.bounds.x,
            frame.bounds.y,
            frame.bounds.width,
            frame.bounds.height,
            (frame.alpha * 255.0).round() as u8,
            W::THREAD_NAME,
            |target, brush| {
                let mut canvas = D2dCanvas::new(
                    target,
                    brush,
                    &state.text,
                    &state.d2d,
                    &mut state.resources,
                    frame.dpi,
                );
                state.model.draw(&mut canvas, now);
            },
        )
    })
    .flatten();
    // Presentation 拥有当帧缓冲；ShowWindow/ULW 重入时窗口状态已可重新借用。
    if let Some(presentation) = presentation {
        presentation.present(W::THREAD_NAME);
    }
}
fn execute<W: WinWindow>(hwnd: HWND, ops: Ops<Action<W>>, now: Instant) {
    for op in ops {
        match op {
            WindowOp::Redraw => redraw::<W>(hwnd, now),
            WindowOp::Hide => {
                with_state::<W, _>(hwnd, |state| state.surface.mark_hidden());
                unsafe {
                    let _ = ShowWindow(hwnd, SW_HIDE);
                }
            }
            WindowOp::RaiseTopmost => unsafe {
                let _ = SetWindowPos(
                    hwnd,
                    HWND_TOPMOST,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                );
            },
            WindowOp::SetBounds(bounds) => unsafe {
                let _ = SetWindowPos(
                    hwnd,
                    HWND_TOPMOST,
                    bounds.x,
                    bounds.y,
                    bounds.width,
                    bounds.height,
                    SWP_NOACTIVATE,
                );
            },
            WindowOp::MoveTo { x, y } => unsafe {
                let _ = SetWindowPos(hwnd, HWND_TOPMOST, x, y, 0, 0, SWP_NOSIZE | SWP_NOACTIVATE);
            },
            WindowOp::SetInteractive(interactive) => unsafe {
                let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
                let next = if interactive {
                    style & !(WS_EX_TRANSPARENT.0 as isize)
                } else {
                    style | (WS_EX_TRANSPARENT.0 as isize)
                };
                if next != style {
                    SetWindowLongPtrW(hwnd, GWL_EXSTYLE, next);
                    let _ = SetWindowPos(
                        hwnd,
                        HWND::default(),
                        0,
                        0,
                        0,
                        0,
                        SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
                    );
                }
            },
            WindowOp::TrackMouseLeave => unsafe {
                let mut track = TRACKMOUSEEVENT {
                    cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                let _ = TrackMouseEvent(&mut track);
            },
            WindowOp::BeginSystemDrag => {
                sync_timer::<W>(hwnd, now);
                unsafe {
                    let _ = ReleaseCapture();
                    SendMessageW(
                        hwnd,
                        WM_NCLBUTTONDOWN,
                        WPARAM(HTCAPTION as usize),
                        LPARAM(0),
                    );
                    let mut rect = RECT::default();
                    if GetWindowRect(hwnd, &mut rect).is_ok() {
                        window_event::<W>(
                            hwnd,
                            WindowEvent::SystemDragEnded {
                                bounds: px_rect(rect),
                            },
                            overlay_now(),
                        );
                    }
                }
            }
            WindowOp::Action(action) => W::dispatch(action),
        }
    }
}
fn pointer_event<W: WinWindow>(hwnd: HWND, event: PointerEvent, now: Instant) {
    let steps = with_state::<W, _>(hwnd, |state| state.pointer.event(event, &state.model.hit()))
        .unwrap_or_default();
    unsafe {
        match event {
            PointerEvent::LeftDown { .. } => {
                SetCapture(hwnd);
            }
            PointerEvent::LeftUp { .. } => {
                let _ = ReleaseCapture();
            }
            _ => {}
        }
    }
    for step in steps {
        match step {
            PointerStep::Op(op) => execute::<W>(hwnd, vec![op], now),
            PointerStep::Model(input) => update::<W>(hwnd, now, |model, cx| model.input(input, cx)),
        }
    }
    finish::<W>(hwnd, now);
}
pub(crate) fn client_point(lparam: LPARAM) -> (i32, i32) {
    ((lparam.0 as i16) as i32, ((lparam.0 >> 16) as i16) as i32)
}
fn cursor_position() -> (i32, i32) {
    let mut point = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut point);
    }
    (point.x, point.y)
}
fn px_rect(rect: RECT) -> PxRect {
    PxRect::new(
        rect.left,
        rect.top,
        rect.right - rect.left,
        rect.bottom - rect.top,
    )
}
pub(crate) fn primary_area(work: bool) -> PxRect {
    let mut area = RECT {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1080,
    };
    unsafe {
        let monitor = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if GetMonitorInfoW(monitor, &mut info).as_bool() {
            area = if work { info.rcWork } else { info.rcMonitor };
        }
    }
    px_rect(area)
}
unsafe extern "system" fn window_proc<W: WinWindow>(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let now = overlay_now();
    match message {
        WM_TIMER if wparam.0 == FRAME_TIMER => {
            let dt =
                with_state::<W, _>(hwnd, |state| state.clock.delta(now)).unwrap_or(FIRST_FRAME_DT);
            update::<W>(hwnd, now, |model, cx| model.on_frame(dt, cx));
        }
        WM_MOUSEMOVE => pointer_event::<W>(
            hwnd,
            PointerEvent::Move {
                client: client_point(lparam),
                screen: cursor_position(),
            },
            now,
        ),
        WM_LBUTTONDOWN => pointer_event::<W>(
            hwnd,
            PointerEvent::LeftDown {
                client: client_point(lparam),
                screen: cursor_position(),
            },
            now,
        ),
        WM_LBUTTONUP => pointer_event::<W>(
            hwnd,
            PointerEvent::LeftUp {
                client: client_point(lparam),
            },
            now,
        ),
        WM_RBUTTONUP => pointer_event::<W>(
            hwnd,
            PointerEvent::RightUp {
                client: client_point(lparam),
            },
            now,
        ),
        WM_MOUSELEAVE_MSG => pointer_event::<W>(hwnd, PointerEvent::Leave, now),
        WM_CAPTURECHANGED => pointer_event::<W>(hwnd, PointerEvent::CaptureLost, now),
        WM_MOVE => {
            let (x, y) = client_point(lparam);
            window_event::<W>(hwnd, WindowEvent::Moved { x, y }, now);
        }
        WM_DPICHANGED => {
            let dpi = (wparam.0 & 0xFFFF) as u32;
            let suggested = if lparam.0 != 0 {
                px_rect(*(lparam.0 as *const RECT))
            } else {
                PxRect::default()
            };
            with_state::<W, _>(hwnd, |state| state.surface.discard_dib());
            window_event::<W>(hwnd, WindowEvent::DpiChanged { dpi, suggested }, now);
        }
        WM_SETCURSOR if (lparam.0 & 0xffff) == HTCLIENT as isize => {
            let cursor = with_state::<W, _>(hwnd, |state| state.pointer.cursor(&state.model.hit()));
            let id = if cursor == Some(CursorShape::Move) {
                IDC_SIZEALL
            } else {
                IDC_ARROW
            };
            if let Ok(cursor) = LoadCursorW(None, id) {
                SetCursor(cursor);
            }
            return LRESULT(1);
        }
        WM_DESTROY => {
            let ptr = SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) as *mut RefCell<Host<W>>;
            if !ptr.is_null() {
                drop(Box::from_raw(ptr));
            }
        }
        _ => return DefWindowProcW(hwnd, message, wparam, lparam),
    }
    LRESULT(0)
}
pub(crate) fn create_window<W: WinWindow>() -> Option<HWND> {
    let result = (|| -> Result<_, String> {
        let d2d = create_d2d_factory()?;
        let text = DWriteText::new()?;
        let dpi = system_dpi();
        let mut model = W::create_model(dpi);
        let area = W::area(dpi);
        let bounds = model
            .prepare_frame(&Cx {
                now: overlay_now(),
                dpi,
                area,
                text: &text,
            })
            .map(|frame| frame.bounds)
            .unwrap_or_default();
        unsafe {
            let instance = GetModuleHandleW(None).map_err(|e| e.to_string())?;
            let class = WNDCLASSW {
                lpfnWndProc: Some(window_proc::<W>),
                hInstance: instance.into(),
                lpszClassName: W::CLASS_NAME,
                ..Default::default()
            };
            if RegisterClassW(&class) == 0 && GetLastError() != ERROR_CLASS_ALREADY_EXISTS {
                return Err(format!("注册窗口类失败：{:?}", GetLastError()));
            }
            let hwnd = CreateWindowExW(
                W::EX_STYLE,
                W::CLASS_NAME,
                W::TITLE,
                WS_POPUP,
                bounds.x,
                bounds.y,
                bounds.width,
                bounds.height,
                None,
                None,
                instance,
                None,
            )
            .map_err(|e| format!("创建窗口失败：{e}"))?;
            let state = Box::new(RefCell::new(Host::<W> {
                model,
                pointer: Default::default(),
                text,
                d2d,
                resources: Default::default(),
                surface: LayeredSurface::new(hwnd),
                clock: FrameClock::new(),
            }));
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(state) as isize);
            Some(hwnd).ok_or_else(|| "创建窗口失败".to_string())
        }
    })();
    result
        .map_err(|error| eprintln!("[{}] {error}", W::THREAD_NAME))
        .ok()
}

#[cfg(test)]
pub(crate) fn frame_timer_active<W: WinWindow>(hwnd: HWND) -> bool {
    with_state::<W, _>(hwnd, |state| state.clock.is_active()).unwrap_or(false)
}
#[cfg(test)]
pub(crate) fn capture<W: WinWindow>(hwnd: HWND) -> Option<super::test_seams::Capture> {
    with_state::<W, _>(hwnd, |state| state.surface.capture()).flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use sayit_overlay_core::canvas::{Canvas, TextEngine};
    use sayit_overlay_core::look::Look;
    use windows::Win32::UI::WindowsAndMessaging::{
        DestroyWindow, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    };

    #[derive(Default)]
    struct TestPointer;
    impl PointerModel for TestPointer {
        type Hit = u32;
        type Input = PointerEvent;
        type Action = usize;
        fn event(&mut self, event: PointerEvent, _: &u32) -> Vec<PointerStep<usize, PointerEvent>> {
            vec![PointerStep::Model(event)]
        }
        fn cursor(&self, _: &u32) -> CursorShape {
            CursorShape::Move
        }
    }
    #[derive(Default)]
    struct TestModel {
        events: Vec<WindowEvent>,
        pointer: Vec<PointerEvent>,
        deltas: Vec<f32>,
    }
    impl<T: TextEngine> OverlayModel<T> for TestModel {
        type Command = usize;
        type Action = usize;
        type Pointer = TestPointer;
        fn apply(&mut self, hwnd: usize, _: &Cx<'_, T>) -> Ops<usize> {
            vec![
                WindowOp::SetBounds(PxRect::new(10, 20, 100, 80)),
                WindowOp::MoveTo { x: 30, y: 40 },
                WindowOp::Action(hwnd),
            ]
        }
        fn input(&mut self, input: PointerEvent, _: &Cx<'_, T>) -> Ops<usize> {
            self.pointer.push(input);
            Vec::new()
        }
        fn window_event(&mut self, event: WindowEvent, _: &Cx<'_, T>) -> Ops<usize> {
            self.events.push(event);
            Vec::new()
        }
        fn set_look(&mut self, _: &Look) -> Ops<usize> {
            Vec::new()
        }
        fn on_frame(&mut self, dt: f32, _: &Cx<'_, T>) -> Ops<usize> {
            self.deltas.push(dt);
            Vec::new()
        }
        fn wants_frames(&self, _: Instant) -> bool {
            true
        }
        fn prepare_frame(&mut self, cx: &Cx<'_, T>) -> Option<FrameSpec> {
            Some(FrameSpec {
                bounds: PxRect::new(0, 0, 64, 64),
                dpi: cx.dpi,
                alpha: 1.0,
            })
        }
        fn draw<C: Canvas<Text = T>>(&self, _: &mut C, _: Instant) {}
        fn hit(&self) -> u32 {
            self.events.len() as u32
        }
    }
    struct TestWindow;
    impl WinWindow for TestWindow {
        type Model = TestModel;
        const CLASS_NAME: PCWSTR = w!("SayItOverlayHostRegression");
        const EX_STYLE: WINDOW_EX_STYLE =
            WINDOW_EX_STYLE(WS_EX_LAYERED.0 | WS_EX_NOACTIVATE.0 | WS_EX_TOOLWINDOW.0);
        fn create_model(_: u32) -> TestModel {
            TestModel::default()
        }
        fn area(_: u32) -> PxRect {
            PxRect::default()
        }
        fn dispatch(hwnd: usize) {
            // 动作回调同步重入窗口过程，必须能重新借用模型。
            unsafe {
                SendMessageW(HWND(hwnd as _), WM_TIMER, WPARAM(FRAME_TIMER), LPARAM(0));
            }
        }
    }
    struct TestWindowGuard(HWND);
    impl Drop for TestWindowGuard {
        fn drop(&mut self) {
            unsafe {
                let _ = DestroyWindow(self.0);
            }
        }
    }
    #[test]
    fn native_host_releases_state_before_reentrant_window_ops_and_actions() {
        let _seams = super::super::test_seams::OffscreenGuard::new(96);
        let window = TestWindowGuard(create_window::<TestWindow>().unwrap());
        apply::<TestWindow>(window.0, window.0 .0 as usize);
        with_state::<TestWindow, _>(window.0, |state| {
            assert_eq!(
                state.model.events,
                vec![
                    WindowEvent::Moved { x: 10, y: 20 },
                    WindowEvent::Moved { x: 30, y: 40 }
                ]
            );
            assert_eq!(state.model.deltas, vec![FIRST_FRAME_DT]);
        });
    }
    #[test]
    fn native_host_routes_pointer_dpi_move_and_cursor_messages() {
        let _seams = super::super::test_seams::OffscreenGuard::new(96);
        let window = TestWindowGuard(create_window::<TestWindow>().unwrap());
        let point = LPARAM(((7_i32 << 16) | 0xfffe) as isize);
        let bounds = RECT {
            left: -10,
            top: 5,
            right: 90,
            bottom: 85,
        };
        unsafe {
            for message in [
                WM_MOUSEMOVE,
                WM_LBUTTONDOWN,
                WM_LBUTTONUP,
                WM_RBUTTONUP,
                WM_MOUSELEAVE_MSG,
                WM_CAPTURECHANGED,
            ] {
                SendMessageW(window.0, message, WPARAM(0), point);
            }
            SendMessageW(
                window.0,
                WM_DPICHANGED,
                WPARAM(144),
                LPARAM(&bounds as *const _ as isize),
            );
            assert_eq!(
                SendMessageW(window.0, WM_SETCURSOR, WPARAM(0), LPARAM(HTCLIENT as isize)),
                LRESULT(1)
            );
        }
        with_state::<TestWindow, _>(window.0, |state| {
            assert!(matches!(
                state.model.pointer[0],
                PointerEvent::Move {
                    client: (-2, 7),
                    ..
                }
            ));
            assert!(state
                .model
                .pointer
                .contains(&PointerEvent::LeftUp { client: (-2, 7) }));
            assert!(state
                .model
                .pointer
                .contains(&PointerEvent::RightUp { client: (-2, 7) }));
            assert!(state.model.pointer.contains(&PointerEvent::Leave));
            assert!(state.model.pointer.contains(&PointerEvent::CaptureLost));
            assert_eq!(
                state.model.events.last(),
                Some(&WindowEvent::DpiChanged {
                    dpi: 144,
                    suggested: PxRect::new(-10, 5, 100, 80)
                })
            );
        });
    }
}
