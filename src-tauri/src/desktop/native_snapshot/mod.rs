//! 原生悬浮窗像素快照：用真实窗口过程驱动场景，离屏截取各关键状态的 DIB，
//! 作为重构前后逐字节比对的安全网。
//!
//! 模式由 `SAYIT_OVERLAY_SNAPSHOTS` 决定：
//! - `record`：写基准 `<case>.png`（预乘 BGRA 只交换 R/B 后按 RGBA 无损保存）与
//!   `manifest.txt`（每行 `case x y w h alpha`）；
//! - `verify`：逐字节比对像素与上屏参数，不一致时写 `<case>.actual.png` 与
//!   `<case>.diff.png`，全部场景跑完后汇总再失败；
//! - 未设置（CI 默认）：只跑场景，断言该有内容的帧确有不透明像素、该透明的帧整窗透明。
//!
//! 基准依赖本机的 D2D 软件光栅、DirectWrite 渲染参数与显示器布局，只在同一台
//! 机器上有效，默认放在已 gitignore 的 `测试/native-overlay-snapshots/<window>/`；
//! `SAYIT_OVERLAY_SNAPSHOT_DIR` 可改指其他目录（worktree 指向主检出的基准）。
//! 场景测试函数名一律以 `native_snapshot_` 开头，过滤器 `native_snapshot` 即可选中全部。

mod indicator;
mod orb;
mod subtitle;

use super::native_overlay::create_dwrite_factory;
use super::native_overlay::test_seams::{self, Capture, OffscreenGuard};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{BOOL, HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    DestroyWindow, SendMessageW, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_TIMER,
};

const MODE_ENV: &str = "SAYIT_OVERLAY_SNAPSHOTS";
const DIR_ENV: &str = "SAYIT_OVERLAY_SNAPSHOT_DIR";
const MANIFEST: &str = "manifest.txt";
/// 三个窗口都用 16ms 的帧定时器。
const FRAME: Duration = Duration::from_millis(16);
/// 场景用到的全部字体；缺失时 DirectWrite 会静默回退，录下的就不是真实效果。
const REQUIRED_FONTS: [&str; 4] = [
    "Microsoft YaHei UI",
    "Microsoft YaHei",
    "Segoe MDL2 Assets",
    "Segoe UI",
];
const MK_LBUTTON: usize = 0x0001;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Smoke,
    Record,
    Verify,
}

impl Mode {
    fn from_env() -> Self {
        match std::env::var(MODE_ENV) {
            Err(_) => Self::Smoke,
            Ok(value) => match value.as_str() {
                "record" => Self::Record,
                "verify" => Self::Verify,
                other => panic!("{MODE_ENV} 只接受 record 或 verify，实际为 {other:?}"),
            },
        }
    }
}

/// 一类窗口的驱动方式，由各窗口模块的 `snapshot_driver` 提供。
#[derive(Clone, Copy)]
pub(super) struct Driver {
    pub(super) create_window: fn() -> HWND,
    pub(super) frame_timer: usize,
    pub(super) frame_timer_active: fn(HWND) -> bool,
    pub(super) capture: fn(HWND) -> Option<Capture>,
}

/// 一类窗口的全部快照：逐个 case 截取与比对，`finish` 时统一落盘清单或汇总失败。
pub(super) struct Snapshots {
    window: &'static str,
    mode: Mode,
    dir: PathBuf,
    /// verify：基准清单，case → 清单行。
    baseline: BTreeMap<String, String>,
    /// record：本次写出的清单行，按场景顺序。
    manifest: Vec<String>,
    seen: BTreeSet<String>,
    failures: Vec<String>,
}

impl Snapshots {
    pub(super) fn new(window: &'static str) -> Self {
        let mode = Mode::from_env();
        let root = std::env::var_os(DIR_ENV)
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("..")
                    .join("测试")
                    .join("native-overlay-snapshots")
            });
        let dir = root.join(window);
        let missing = missing_fonts();
        match mode {
            Mode::Smoke if !missing.is_empty() => {
                eprintln!(
                    "[native-snapshot] 缺少字体 {missing:?}，冒烟模式继续，文字可能走回退字体"
                );
            }
            Mode::Smoke => {}
            Mode::Record | Mode::Verify => {
                assert!(missing.is_empty(), "快照依赖的字体不存在：{missing:?}");
            }
        }
        let mut baseline = BTreeMap::new();
        if mode != Mode::Smoke {
            std::fs::create_dir_all(&dir)
                .unwrap_or_else(|error| panic!("创建快照目录 {} 失败：{error}", dir.display()));
            remove_failure_artifacts(&dir);
        }
        if mode == Mode::Verify {
            let path = dir.join(MANIFEST);
            let text = std::fs::read_to_string(&path).unwrap_or_else(|error| {
                panic!(
                    "读取基准清单 {} 失败（需先 record）：{error}",
                    path.display()
                )
            });
            for line in text.lines().filter(|line| !line.trim().is_empty()) {
                let case = line.split(' ').next().unwrap_or_default().to_string();
                baseline.insert(case, line.to_string());
            }
        }
        Self {
            window,
            mode,
            dir,
            baseline,
            manifest: Vec::new(),
            seen: BTreeSet::new(),
            failures: Vec::new(),
        }
    }

    /// 在指定 DPI 下新建一个离屏窗口；场景结束时随会话一起销毁。
    pub(super) fn session(&mut self, driver: Driver, dpi: u32) -> Session<'_> {
        // DPI 覆盖必须先于建窗：建窗时的初始布局也读它。
        let seams = OffscreenGuard::new(dpi);
        let t0 = Instant::now();
        test_seams::set_now(t0);
        let hwnd = (driver.create_window)();
        Session {
            snaps: self,
            driver,
            dpi,
            hwnd,
            t0,
            elapsed: Duration::ZERO,
            _seams: seams,
        }
    }

    fn check(&mut self, case: String, capture: Option<Capture>, transparent: bool) {
        assert!(self.seen.insert(case.clone()), "快照 case 重名：{case}");
        let Some(capture) = capture else {
            self.failures
                .push(format!("{case}：窗口未上屏（已隐藏或尚未渲染）"));
            return;
        };
        let presented = capture.presented;
        let label = format!("{case}（第 {} 帧）", presented.frame);
        let (width, height) = (presented.width as u32, presented.height as u32);
        if capture.pixels.len() != width as usize * height as usize * 4 {
            self.failures.push(format!(
                "{label}：DIB 字节数 {} 与上屏尺寸 {width}x{height} 不符",
                capture.pixels.len()
            ));
            return;
        }
        let opaque = capture
            .pixels
            .chunks_exact(4)
            .filter(|px| px[3] != 0)
            .count();
        if transparent && opaque != 0 {
            self.failures
                .push(format!("{label}：应整窗透明，实际有 {opaque} 个不透明像素"));
        } else if !transparent && opaque == 0 {
            self.failures
                .push(format!("{label}：应有内容，实际整窗透明"));
        }
        let line = format!(
            "{case} {} {} {width} {height} {}",
            presented.x, presented.y, presented.alpha
        );
        let rgba = swap_red_blue(&capture.pixels);
        match self.mode {
            Mode::Smoke => {}
            Mode::Record => {
                let path = self.png_path(&case, "");
                save_png(&path, &rgba, width, height);
                self.manifest.push(line);
            }
            Mode::Verify => self.compare(&case, &line, &rgba, width, height),
        }
    }

    fn compare(&mut self, case: &str, line: &str, rgba: &[u8], width: u32, height: u32) {
        let Some(expected_line) = self.baseline.get(case) else {
            self.failures
                .push(format!("{case}：基准清单里没有这个 case"));
            return;
        };
        if expected_line != line {
            self.failures.push(format!(
                "{case}：上屏参数不一致，基准 `{expected_line}`，实际 `{line}`"
            ));
        }
        let path = self.png_path(case, "");
        let expected = match image::open(&path) {
            Ok(image) => image.into_rgba8(),
            Err(error) => {
                self.failures
                    .push(format!("{case}：读取基准 {} 失败：{error}", path.display()));
                return;
            }
        };
        if expected.dimensions() != (width, height) {
            save_png(&self.png_path(case, ".actual"), rgba, width, height);
            self.failures.push(format!(
                "{case}：尺寸不一致，基准 {:?}，实际 {width}x{height}",
                expected.dimensions()
            ));
            return;
        }
        let mut differing = 0usize;
        let mut max_delta = 0u8;
        let mut diff = Vec::with_capacity(rgba.len());
        for (actual, base) in rgba.chunks_exact(4).zip(expected.as_raw().chunks_exact(4)) {
            let delta = actual
                .iter()
                .zip(base)
                .map(|(a, b)| a.abs_diff(*b))
                .max()
                .unwrap_or(0);
            if delta == 0 {
                // 未变像素压暗保留轮廓，差异像素标红。
                diff.extend_from_slice(&[base[0], base[1], base[2], base[3] / 4]);
            } else {
                differing += 1;
                max_delta = max_delta.max(delta);
                diff.extend_from_slice(&[255, 0, 0, 255]);
            }
        }
        if differing > 0 {
            save_png(&self.png_path(case, ".actual"), rgba, width, height);
            save_png(&self.png_path(case, ".diff"), &diff, width, height);
            self.failures.push(format!(
                "{case}：{differing} 个像素不同，最大通道差 {max_delta}"
            ));
        }
    }

    /// 写出清单或汇总比对结果；任何失败都在这里统一报告。
    pub(super) fn finish(mut self) {
        match self.mode {
            Mode::Smoke => {}
            Mode::Record => {
                let path = self.dir.join(MANIFEST);
                let mut text = self.manifest.join("\n");
                text.push('\n');
                std::fs::write(&path, text)
                    .unwrap_or_else(|error| panic!("写入 {} 失败：{error}", path.display()));
            }
            Mode::Verify => {
                let missing: Vec<String> = self
                    .baseline
                    .keys()
                    .filter(|case| !self.seen.contains(*case))
                    .cloned()
                    .collect();
                for case in missing {
                    self.failures.push(format!("{case}：基准里有，场景里没有"));
                }
            }
        }
        assert!(
            self.failures.is_empty(),
            "{} 快照（{:?}，目录 {}）有 {} 处失败：\n{}",
            self.window,
            self.mode,
            self.dir.display(),
            self.failures.len(),
            self.failures.join("\n")
        );
    }

    fn png_path(&self, case: &str, suffix: &str) -> PathBuf {
        self.dir.join(format!("{case}{suffix}.png"))
    }
}

/// 一个离屏窗口加一条注入的测试时钟；帧只在窗口自己的帧定时器开着时派发，
/// 与真实 SetTimer 的触发条件一致。
pub(super) struct Session<'a> {
    snaps: &'a mut Snapshots,
    driver: Driver,
    dpi: u32,
    hwnd: HWND,
    t0: Instant,
    elapsed: Duration,
    // 字段按声明顺序析构：Drop 里先销毁窗口，最后才恢复接缝。
    _seams: OffscreenGuard,
}

impl Session<'_> {
    pub(super) fn hwnd(&self) -> HWND {
        self.hwnd
    }

    pub(super) fn dpi(&self) -> u32 {
        self.dpi
    }

    /// 时钟前进 `ms` 毫秒：按 16ms 一帧推进，最后不足一帧的尾段落在目标时刻。
    pub(super) fn advance(&mut self, ms: u64) {
        let target = self.elapsed + Duration::from_millis(ms);
        while self.elapsed < target {
            self.elapsed = (self.elapsed + FRAME).min(target);
            test_seams::set_now(self.t0 + self.elapsed);
            if (self.driver.frame_timer_active)(self.hwnd) {
                self.send(WM_TIMER, self.driver.frame_timer, 0);
            }
        }
    }

    /// 在当前时刻触发一次指定的一次性定时器（如胶囊通知到期）。
    pub(super) fn fire_timer(&mut self, id: usize) {
        self.send(WM_TIMER, id, 0);
    }

    /// 指针移到布局坐标（DIP）处，按窗口 DPI 换成客户区物理像素。
    pub(super) fn mouse_move(&mut self, point: (f32, f32)) {
        let lparam = self.client_lparam(point);
        self.send(WM_MOUSEMOVE, 0, lparam);
    }

    pub(super) fn click(&mut self, point: (f32, f32)) {
        let lparam = self.client_lparam(point);
        self.send(WM_LBUTTONDOWN, MK_LBUTTON, lparam);
        self.send(WM_LBUTTONUP, 0, lparam);
    }

    /// 截取当前帧，case 名自动带上 `@dpi`。
    pub(super) fn shot(&mut self, case: &str) {
        self.check(case, false);
    }

    /// 截取一帧本应整窗透明的状态（如悬浮球定位态）。
    pub(super) fn shot_transparent(&mut self, case: &str) {
        self.check(case, true);
    }

    fn check(&mut self, case: &str, transparent: bool) {
        let capture = (self.driver.capture)(self.hwnd);
        let case = format!("{case}@{}", self.dpi);
        self.snaps.check(case, capture, transparent);
    }

    fn client_lparam(&self, (x, y): (f32, f32)) -> isize {
        let scale = self.dpi as f32 / 96.0;
        let x = (x * scale).round() as i32;
        let y = (y * scale).round() as i32;
        ((y & 0xFFFF) << 16 | (x & 0xFFFF)) as isize
    }

    fn send(&self, message: u32, wparam: usize, lparam: isize) {
        // 同线程 SendMessageW 直接调用窗口过程，不经过消息队列。
        unsafe {
            SendMessageW(self.hwnd, message, WPARAM(wparam), LPARAM(lparam));
        }
    }
}

impl Drop for Session<'_> {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

fn swap_red_blue(bgra: &[u8]) -> Vec<u8> {
    let mut rgba = bgra.to_vec();
    for pixel in rgba.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    rgba
}

fn save_png(path: &Path, rgba: &[u8], width: u32, height: u32) {
    image::save_buffer(path, rgba, width, height, image::ColorType::Rgba8)
        .unwrap_or_else(|error| panic!("写入 {} 失败：{error}", path.display()));
}

/// 上一次 verify 留下的 `.actual.png` / `.diff.png` 会误导排查，每次开跑前清掉。
fn remove_failure_artifacts(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.ends_with(".actual.png") || name.ends_with(".diff.png") {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

fn missing_fonts() -> Vec<&'static str> {
    let dwrite = create_dwrite_factory().expect("创建 DirectWrite 工厂失败");
    let mut collection = None;
    unsafe { dwrite.GetSystemFontCollection(&mut collection, BOOL::from(false)) }
        .expect("读取系统字体集合失败");
    let collection = collection.expect("系统字体集合为空");
    REQUIRED_FONTS
        .into_iter()
        .filter(|family| {
            let wide: Vec<u16> = family.encode_utf16().chain(std::iter::once(0)).collect();
            let mut index = 0u32;
            let mut exists = BOOL::from(false);
            let found = unsafe {
                collection.FindFamilyName(PCWSTR(wide.as_ptr()), &mut index, &mut exists)
            };
            found.is_err() || !exists.as_bool()
        })
        .collect()
}
