//! 动态上下文的 Windows 采集。听写期间两个线程观察用户的操作：一个每 33ms 看光标、鼠标键盘和
//! 前台窗口，认出切换窗口、选中文字和鼠标指向；一个每 500ms 截一张前台窗口的缩略图，认出滚动、
//! 打开图片这类内容变化。认出后在后台线程里读取对应的文字。
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, GetDoubleClickTime, VK_LBUTTON, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetAncestor, GetCursorPos, GetWindowThreadProcessId, WindowFromPoint, GA_ROOT,
};

use super::dynamic::{
    pointed_block, thumbnail_changed, DynamicEntry, DynamicKind, PointerTracker, POINTER_REGION,
};
use super::model::{
    ActivationTarget, ActiveAppContextExtractionMethod, CaptureOptions, CaptureStatus,
};
use super::windows::WindowsActiveAppContextProvider;
use super::{ocr, screen_capture, ActiveAppContextProvider};
use crate::providers::capabilities::OcrProvider;

const TICK: Duration = Duration::from_millis(33);
/// 每隔几个采样周期看一次前台窗口（约 100ms）。
const FOREGROUND_EVERY: u32 = 3;
/// 截一张缩略图约要 40ms（主要是从屏幕读回像素），所以单独一个线程、隔得久一些。
const THUMBNAIL_INTERVAL: Duration = Duration::from_millis(500);
/// 切到新窗口后等它画完再读。
const SWITCH_SETTLE: Duration = Duration::from_millis(400);
/// 松开鼠标或 Shift 后等选区稳定再读。
const SELECTION_SETTLE: Duration = Duration::from_millis(150);
/// 拖动超过这么多像素才算拖选。
const DRAG_MIN: i32 = 8;
/// 一次听写最多补读几次窗口文字（OCR 每次要几百毫秒处理器时间）、几次鼠标指向和选区。
const MAX_SNAPSHOTS: usize = 5;
const MAX_POINTERS: usize = 8;
const MAX_SELECTIONS: usize = 8;
const POINTER_OCR_TIMEOUT: Duration = Duration::from_millis(1500);
/// 听写结束时最多再等这么久，让正在读的那一次赶上。
const FINISH_WAIT: Duration = Duration::from_millis(150);

pub(crate) struct DynamicMonitor {
    shared: Arc<Shared>,
}

impl Drop for DynamicMonitor {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
        self.shared.closed.store(true, Ordering::Release);
    }
}

struct Shared {
    started: Instant,
    /// 不再观察用户操作。
    stop: AtomicBool,
    /// 不再收新结果，正在读的也中止。
    closed: Arc<AtomicBool>,
    entries: Mutex<Vec<DynamicEntry>>,
    /// 识别文字的进度：（毫秒数，已识别的字数）。
    spoken: Mutex<Vec<(u64, usize)>>,
    busy: AtomicUsize,
    blocked_apps: Vec<String>,
    method: ActiveAppContextExtractionMethod,
    /// 当前前台窗口，两个观察线程共用。
    window: AtomicIsize,
    snapshots: AtomicUsize,
    pointers: AtomicUsize,
    selections: AtomicUsize,
    snapshot_slot: Arc<AtomicBool>,
    selection_slot: Arc<AtomicBool>,
    pointer_slot: Arc<AtomicBool>,
}

impl Shared {
    fn watching(&self) -> bool {
        !self.stop.load(Ordering::Acquire) && !self.closed.load(Ordering::Acquire)
    }

    fn record(&self, at_ms: u64, kind: DynamicKind) {
        if self.closed.load(Ordering::Acquire) {
            return;
        }
        if let Ok(mut entries) = self.entries.lock() {
            entries.push(DynamicEntry { at_ms, kind });
        }
    }

    /// 和上一条同类记录内容相同时不再记。
    fn record_new(&self, at_ms: u64, kind: DynamicKind) {
        let repeated = self.entries.lock().is_ok_and(|entries| {
            entries
                .iter()
                .rev()
                .find(|entry| std::mem::discriminant(&entry.kind) == std::mem::discriminant(&kind))
                .is_some_and(|entry| entry.kind == kind)
        });
        if !repeated {
            self.record(at_ms, kind);
        }
    }

    fn elapsed_ms(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }

    /// 在后台线程里跑一件读取任务；`slot` 为真表示同类任务正在跑，这次跳过。返回是否开始跑了。
    fn spawn(self: &Arc<Self>, slot: &Arc<AtomicBool>, job: impl FnOnce(&Shared) + Send + 'static) -> bool {
        if slot.swap(true, Ordering::AcqRel) {
            return false;
        }
        self.busy.fetch_add(1, Ordering::AcqRel);
        let release = {
            let (shared, slot) = (Arc::clone(self), Arc::clone(slot));
            move || {
                shared.busy.fetch_sub(1, Ordering::AcqRel);
                slot.store(false, Ordering::Release);
            }
        };
        let shared = Arc::clone(self);
        let spawned = std::thread::Builder::new()
            .name("dynamic-context-read".into())
            .spawn({
                let release = release.clone();
                move || {
                    job(&shared);
                    release();
                }
            });
        if spawned.is_err() {
            release();
            return false;
        }
        true
    }

    /// 同类读取次数没用完、上一次也读完了才开始读；返回是否开始读了。
    fn spawn_limited(
        self: &Arc<Self>,
        slot: &Arc<AtomicBool>,
        count: &AtomicUsize,
        limit: usize,
        job: impl FnOnce(&Shared) + Send + 'static,
    ) -> bool {
        if count.load(Ordering::Acquire) >= limit {
            return false;
        }
        let started = self.spawn(slot, job);
        if started {
            count.fetch_add(1, Ordering::AcqRel);
        }
        started
    }

    fn snapshot(self: &Arc<Self>, target: ActivationTarget, switched: bool) -> bool {
        self.spawn_limited(&self.snapshot_slot, &self.snapshots, MAX_SNAPSHOTS, move |shared| {
            read_window(shared, target, switched)
        })
    }
}

impl DynamicMonitor {
    pub(crate) fn start(
        target: ActivationTarget,
        blocked_apps: Vec<String>,
        method: ActiveAppContextExtractionMethod,
    ) -> Self {
        let shared = Arc::new(Shared {
            started: Instant::now(),
            stop: AtomicBool::new(false),
            closed: Arc::new(AtomicBool::new(false)),
            entries: Mutex::new(Vec::new()),
            spoken: Mutex::new(Vec::new()),
            busy: AtomicUsize::new(0),
            blocked_apps,
            method,
            window: AtomicIsize::new(target.window_handle),
            snapshots: AtomicUsize::new(0),
            pointers: AtomicUsize::new(0),
            selections: AtomicUsize::new(0),
            snapshot_slot: Arc::new(AtomicBool::new(false)),
            selection_slot: Arc::new(AtomicBool::new(false)),
            pointer_slot: Arc::new(AtomicBool::new(false)),
        });
        for (name, watcher) in [
            ("dynamic-context-input", watch_input as fn(Arc<Shared>, ActivationTarget)),
            ("dynamic-context-content", watch_content),
        ] {
            let shared = Arc::clone(&shared);
            let _ = std::thread::Builder::new()
                .name(name.into())
                .spawn(move || watcher(shared, target));
        }
        Self { shared }
    }

    /// 不再观察用户操作；已经在读的照常读完。
    pub(crate) fn stop(&self) {
        self.shared.stop.store(true, Ordering::Release);
    }

    /// 置位后不再收新结果，正在读的也中止：用户取消听写时用。
    pub(crate) fn closed(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.shared.closed)
    }

    /// 停止观察后的字数不再记：那是收尾时的重新识别，时间不是说话的时间。
    pub(crate) fn note_transcript(&self, chars: usize) {
        if !self.shared.watching() {
            return;
        }
        let at_ms = self.shared.elapsed_ms();
        if let Ok(mut spoken) = self.shared.spoken.lock() {
            if spoken.last().is_none_or(|(_, last)| *last != chars) {
                spoken.push((at_ms, chars));
            }
        }
    }

    /// 到目前为止记下的操作。
    pub(crate) fn entries(&self) -> Vec<DynamicEntry> {
        self.shared.entries.lock().map(|entries| entries.clone()).unwrap_or_default()
    }

    /// 停止观察，给正在读的那一次一点时间赶上，再返回全部记录和识别进度。
    pub(crate) async fn finish(self) -> (Vec<DynamicEntry>, Vec<(u64, usize)>) {
        self.shared.stop.store(true, Ordering::Release);
        let deadline = Instant::now() + FINISH_WAIT;
        while self.shared.busy.load(Ordering::Acquire) > 0 && Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        self.shared.closed.store(true, Ordering::Release);
        let spoken = self.shared.spoken.lock().map(|spoken| spoken.clone()).unwrap_or_default();
        (self.entries(), spoken)
    }
}

/// 光标、鼠标键盘和前台窗口。
fn watch_input(shared: Arc<Shared>, initial: ActivationTarget) {
    let mut current = initial;
    let mut tracker = PointerTracker::new(cursor().unwrap_or_default());
    let mut switch_due: Option<Instant> = None;
    let mut left_down: Option<(i32, i32)> = None;
    let mut last_click = Instant::now() - Duration::from_secs(10);
    let mut shift_down = false;
    let mut selection_due: Option<Instant> = None;
    let double_click = Duration::from_millis(unsafe { GetDoubleClickTime() } as u64);
    let mut tick = 0u32;
    while shared.watching() {
        std::thread::sleep(TICK);
        tick = tick.wrapping_add(1);
        let now = Instant::now();
        let at_ms = shared.elapsed_ms();
        let position = cursor();

        if let Some((x, y)) = position {
            if let Some(point) = tracker.push(at_ms, x, y) {
                shared.spawn_limited(&shared.pointer_slot, &shared.pointers, MAX_POINTERS, move |shared| {
                    read_pointer(shared, at_ms, point)
                });
            }
        }

        let left = key_down(VK_LBUTTON.0);
        match (left_down, left, position) {
            (None, true, Some(point)) => left_down = Some(point),
            (Some(start), false, end) => {
                left_down = None;
                let dragged = end.is_some_and(|end| (end.0 - start.0).abs().max((end.1 - start.1).abs()) > DRAG_MIN);
                if dragged || now - last_click < double_click {
                    selection_due = Some(now + SELECTION_SETTLE);
                }
                last_click = now;
            }
            _ => {}
        }
        let shift = key_down(VK_SHIFT.0);
        if shift_down && !shift {
            selection_due = Some(now + SELECTION_SETTLE);
        }
        shift_down = shift;
        if selection_due.is_some_and(|due| now >= due) {
            selection_due = None;
            if let Some(target) = super::windows::activation_target() {
                shared.spawn_limited(&shared.selection_slot, &shared.selections, MAX_SELECTIONS, move |shared| {
                    read_selection(shared, target)
                });
            }
        }

        if tick % FOREGROUND_EVERY == 0 {
            if let Some(target) = super::windows::activation_target() {
                if target.window_handle != current.window_handle {
                    current = target;
                    shared.window.store(target.window_handle, Ordering::Release);
                    switch_due = Some(now + SWITCH_SETTLE);
                }
            }
        }
        // 上一次还在读时等它读完：切换后的窗口总要读到。
        if switch_due.is_some_and(|due| now >= due) && shared.snapshot(current, true) {
            switch_due = None;
        }
    }
}

/// 前台窗口的内容变化：缩略图和上次读取时比明显变了、而且不再变（滚动停下了），才读一次。
/// 切到新窗口后由切换那次读取负责，这里只把新窗口的样子记作比较基准。
fn watch_content(shared: Arc<Shared>, initial: ActivationTarget) {
    let mut window = initial.window_handle;
    let mut baseline = screen_capture::window_thumbnail(window).ok();
    let mut last = baseline.clone();
    loop {
        std::thread::sleep(THUMBNAIL_INTERVAL);
        if !shared.watching() {
            break;
        }
        let current = shared.window.load(Ordering::Acquire);
        let thumbnail = screen_capture::window_thumbnail(current).ok();
        if current != window {
            window = current;
            baseline = thumbnail.clone();
            last = thumbnail;
            continue;
        }
        let settled = thumbnail.is_some() && thumbnail == last;
        let changed = match (&thumbnail, &baseline) {
            (Some(thumbnail), Some(baseline)) => thumbnail_changed(thumbnail, baseline),
            _ => false,
        };
        if settled && changed {
            let target = ActivationTarget {
                window_handle: window,
                process_id: process_of(HWND(window as *mut _)),
                cursor_position: cursor(),
            };
            if target.process_id != 0 && shared.snapshot(target, false) {
                baseline = thumbnail.clone();
            }
        }
        last = thumbnail;
    }
}

fn read_window(shared: &Shared, target: ActivationTarget, switched: bool) {
    let at_ms = shared.elapsed_ms();
    let mut options = CaptureOptions::for_method(shared.method, OcrProvider::default_local());
    options.copy_selection = false;
    let context = WindowsActiveAppContextProvider.capture(target, &shared.blocked_apps, options, &shared.closed);
    if context.status != CaptureStatus::Captured {
        return;
    }
    shared.record(
        at_ms,
        DynamicKind::Window {
            switched,
            app_name: context.app_name.clone(),
            window_title: context.window_title.clone(),
            lines: context.text_lines(),
        },
    );
}

fn read_selection(shared: &Shared, target: ActivationTarget) {
    let at_ms = shared.elapsed_ms();
    let mut options = CaptureOptions::for_method(
        ActiveAppContextExtractionMethod::NativeText,
        OcrProvider::default_local(),
    );
    options.selection_only = true;
    options.copy_selection = false;
    let context = WindowsActiveAppContextProvider.capture(target, &shared.blocked_apps, options, &shared.closed);
    if let Some(text) = context
        .selected_text
        .filter(|text| context.status == CaptureStatus::Captured && !text.trim().is_empty())
    {
        shared.record_new(at_ms, DynamicKind::Selection { text });
    }
}

/// 截取光标周围一块区域识别文字，取光标所在或离它最近的那一段。区域限制在光标所在的窗口里，
/// 不截到旁边的窗口；那个窗口在黑名单里、或焦点在密码框时不截。
fn read_pointer(shared: &Shared, at_ms: u64, (x, y): (i32, i32)) {
    let Some(target) = window_at(x, y) else { return };
    let baseline = super::windows::baseline_context(target, &shared.blocked_apps, shared.method);
    if baseline.status == CaptureStatus::Blocked
        || super::windows::focused_target_is_password(target).unwrap_or(true)
        || shared.closed.load(Ordering::Acquire)
    {
        return;
    }
    let (width, height) = POINTER_REGION;
    let region = RECT { left: x - width / 2, top: y - height / 2, right: x + width / 2, bottom: y + height / 2 };
    let Ok((image, region)) = screen_capture::capture_window_region(target.window_handle, region) else {
        return;
    };
    let at = (
        (x - region.left) as f32 / (region.right - region.left) as f32,
        (y - region.top) as f32 / (region.bottom - region.top) as f32,
    );
    let size = ((region.right - region.left) as f32, (region.bottom - region.top) as f32);
    let Ok(output) = ocr::run_full_window(
        OcrProvider::default_local(),
        image,
        Instant::now() + POINTER_OCR_TIMEOUT,
        Arc::clone(&shared.closed),
    ) else {
        return;
    };
    if let Some(text) = pointed_block(&output.blocks, at, size) {
        shared.record_new(at_ms, DynamicKind::Pointer { text });
    }
}

fn window_at(x: i32, y: i32) -> Option<ActivationTarget> {
    let window = unsafe { GetAncestor(WindowFromPoint(POINT { x, y }), GA_ROOT) };
    let process_id = process_of(window);
    (!window.0.is_null() && process_id != 0 && process_id != std::process::id()).then_some(ActivationTarget {
        window_handle: window.0 as isize,
        process_id,
        cursor_position: Some((x, y)),
    })
}

fn process_of(window: HWND) -> u32 {
    let mut process_id = 0u32;
    if !window.0.is_null() {
        unsafe { GetWindowThreadProcessId(window, Some(&mut process_id)) };
    }
    process_id
}

fn cursor() -> Option<(i32, i32)> {
    let mut point = POINT::default();
    unsafe { GetCursorPos(&mut point) }.is_ok().then_some((point.x, point.y))
}

fn key_down(key: u16) -> bool {
    (unsafe { GetAsyncKeyState(key as i32) } as u16 & 0x8000) != 0
}
