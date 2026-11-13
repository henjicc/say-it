use std::cell::RefCell;
use std::time::{Duration, Instant};

use enigo::{Direction, Enigo, Key, Keyboard, Settings as EnigoSettings};
use windows::core::w;
use windows::Win32::Foundation::{
    GlobalFree, SetLastError, BOOL, HANDLE, HWND, LPARAM, LRESULT, WIN32_ERROR, WPARAM,
};
use windows::Win32::Graphics::Gdi::{HBITMAP, HENHMETAFILE};
use windows::Win32::System::Com::{
    STGMEDIUM, STGMEDIUM_0, TYMED_ENHMF, TYMED_GDI, TYMED_HGLOBAL, TYMED_MFPICT,
};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData, GetClipboardOwner,
    OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Ole::{
    OleDuplicateData, OleInitialize, OleUninitialize, ReleaseStgMedium, CF_BITMAP, CF_DSPBITMAP,
    CF_DSPENHMETAFILE, CF_DSPMETAFILEPICT, CF_ENHMETAFILE, CF_GDIOBJFIRST, CF_GDIOBJLAST,
    CF_METAFILEPICT, CF_PALETTE, CF_UNICODETEXT, CLIPBOARD_FORMAT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, MsgWaitForMultipleObjects, PeekMessageW,
    RegisterClassW, HMENU, HWND_MESSAGE, MSG, PM_REMOVE, QS_ALLINPUT, WINDOW_EX_STYLE,
    WINDOW_STYLE, WM_RENDERALLFORMATS, WM_RENDERFORMAT, WNDCLASSW,
};

const CLIPBOARD_OPEN_ATTEMPTS: usize = 10;
const CLIPBOARD_OPEN_RETRY_DELAY: Duration = Duration::from_millis(5);

struct OleGuard;
impl Drop for OleGuard {
    fn drop(&mut self) {
        unsafe { OleUninitialize() }
    }
}

struct ClipboardGuard;

impl ClipboardGuard {
    /// `owner` 为空时清空后的剪贴板没有所有者；粘贴时传入粘贴窗口，读取请求才会发给它。
    fn open(owner: HWND) -> Result<Self, String> {
        let mut last_error = None;
        for _ in 0..CLIPBOARD_OPEN_ATTEMPTS {
            match unsafe { OpenClipboard(owner) } {
                Ok(()) => return Ok(Self),
                Err(error) => {
                    last_error = Some(error);
                    // 占着剪贴板的程序可能正等粘贴窗口交出数据，等待时要照常处理消息。
                    pump_messages_until(Instant::now() + CLIPBOARD_OPEN_RETRY_DELAY, || false);
                }
            }
        }
        Err(format!(
            "打开 Windows 剪贴板失败：{}",
            last_error
                .map(|error| error.to_string())
                .unwrap_or_else(|| "未知错误".into())
        ))
    }
}

impl Drop for ClipboardGuard {
    fn drop(&mut self) {
        let _ = unsafe { CloseClipboard() };
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ClipboardMedium {
    Global,
    Gdi,
    MetafilePicture,
    EnhancedMetafile,
}

impl ClipboardMedium {
    fn for_format(format: u32) -> Self {
        let format = format as u16;
        if format == CF_METAFILEPICT.0 || format == CF_DSPMETAFILEPICT.0 {
            Self::MetafilePicture
        } else if format == CF_ENHMETAFILE.0 || format == CF_DSPENHMETAFILE.0 {
            Self::EnhancedMetafile
        } else if format == CF_BITMAP.0
            || format == CF_PALETTE.0
            || format == CF_DSPBITMAP.0
            || (CF_GDIOBJFIRST.0..=CF_GDIOBJLAST.0).contains(&format)
        {
            Self::Gdi
        } else {
            Self::Global
        }
    }
}

struct OwnedClipboardFormat {
    format: u32,
    handle: HANDLE,
    medium: ClipboardMedium,
}

impl OwnedClipboardFormat {
    fn duplicate(format: u32, source: HANDLE) -> Result<Self, String> {
        let handle =
            unsafe { OleDuplicateData(source, CLIPBOARD_FORMAT(format as u16), GMEM_MOVEABLE) };
        if handle.is_invalid() {
            return Err(format!("复制 Windows 剪贴板格式 {format} 失败"));
        }
        Ok(Self {
            format,
            handle,
            medium: ClipboardMedium::for_format(format),
        })
    }

    fn transfer_to_clipboard(&mut self) -> Result<(), String> {
        unsafe { SetClipboardData(self.format, self.handle) }
            .map_err(|error| format!("恢复 Windows 剪贴板格式 {} 失败：{error}", self.format))?;
        self.handle = HANDLE::default();
        Ok(())
    }
}

impl Drop for OwnedClipboardFormat {
    fn drop(&mut self) {
        if self.handle.is_invalid() {
            return;
        }
        let mut medium = STGMEDIUM::default();
        medium.tymed = match self.medium {
            ClipboardMedium::Global => {
                medium.u = STGMEDIUM_0 {
                    hGlobal: windows::Win32::Foundation::HGLOBAL(self.handle.0),
                };
                TYMED_HGLOBAL.0 as u32
            }
            ClipboardMedium::Gdi => {
                medium.u = STGMEDIUM_0 {
                    hBitmap: HBITMAP(self.handle.0),
                };
                TYMED_GDI.0 as u32
            }
            ClipboardMedium::MetafilePicture => {
                medium.u = STGMEDIUM_0 {
                    hMetaFilePict: self.handle.0,
                };
                TYMED_MFPICT.0 as u32
            }
            ClipboardMedium::EnhancedMetafile => {
                medium.u = STGMEDIUM_0 {
                    hEnhMetaFile: HENHMETAFILE(self.handle.0),
                };
                TYMED_ENHMF.0 as u32
            }
        };
        unsafe { ReleaseStgMedium(&mut medium) };
        self.handle = HANDLE::default();
    }
}

struct ClipboardSnapshot {
    formats: Vec<OwnedClipboardFormat>,
}

/// 备份剪贴板时，单个格式失败只跳过它。
///
/// 剪贴板里经常有拿不到或复制不了的格式（延迟渲染的格式、源进程已退出的 GDI
/// 对象等）。以前这里一律 `?`，一个格式失败就放弃整份备份，而 `paste_text` 在备份失败时
/// 直接返回错误——听写结果因此完全无法注入。备份里少一种格式，总好过整条注入路径不可用。
/// context-probe 的 C++ 移植版一直就是跳过式的（`if (!source) continue;`）。
fn duplicate_available<T>(
    formats: &[u32],
    mut duplicate: impl FnMut(u32) -> Result<T, String>,
) -> Vec<T> {
    formats
        .iter()
        .filter_map(|format| duplicate(*format).ok())
        .collect()
}

/// 还原时同样不中途放弃：尽量把每一种格式都放回去，再报第一条失败原因。
/// 半途 `?` 返回会把剩下的格式丢掉，用户的剪贴板反而缺得更多。
fn transfer_all<T>(
    items: &mut [T],
    mut transfer: impl FnMut(&mut T) -> Result<(), String>,
) -> Result<(), String> {
    let mut first_error = None;
    for item in items {
        if let Err(error) = transfer(item) {
            first_error.get_or_insert(error);
        }
    }
    first_error.map_or(Ok(()), Err)
}

impl ClipboardSnapshot {
    fn capture() -> Result<Self, String> {
        let _clipboard = ClipboardGuard::open(HWND::default())?;
        let mut available = Vec::new();
        let mut format = 0;
        loop {
            format = unsafe { EnumClipboardFormats(format) };
            if format == 0 {
                break;
            }
            available.push(format);
        }
        let formats = duplicate_available(&available, |format| {
            let source = unsafe { GetClipboardData(format) }
                .map_err(|error| format!("读取 Windows 剪贴板格式 {format} 失败：{error}"))?;
            OwnedClipboardFormat::duplicate(format, source)
        });
        Ok(Self { formats })
    }

    fn restore(mut self) -> Result<(), String> {
        let _clipboard = ClipboardGuard::open(HWND::default())?;
        unsafe { EmptyClipboard() }
            .map_err(|error| format!("清空临时 Windows 剪贴板失败：{error}"))?;
        transfer_all(&mut self.formats, |format| format.transfer_to_clipboard())
    }
}

/// 粘贴线程从备份到还原完成一直持有这把锁；其他要读写剪贴板的地方先调用
/// `wait_clipboard_restored`，免得读到注入的文字，或被随后的还原覆盖。
/// 所有粘贴在专用线程上排队，下一次粘贴备份剪贴板时，上一次的还原一定已经做完；
/// 调用方在按下粘贴键后就返回，胶囊与结束音不必等还原。
static CLIPBOARD_RESTORE: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub(crate) fn wait_clipboard_restored() {
    drop(CLIPBOARD_RESTORE.lock().unwrap_or_else(|poisoned| poisoned.into_inner()));
}

/// 目标软件读到文字之前不能还原，否则它读到的是用户原来的内容，听写结果丢失。目标软件繁忙或
/// 刚启动时，读取可能远晚于按键，没有可靠的固定等待时长。所以文字用延迟渲染交出：剪贴板上先只
/// 登记格式，谁来读，系统才向粘贴窗口要数据（`WM_RENDERFORMAT`），借此知道目标软件读过了。
/// 一直没人读时最多等这么久再还原。
const PASTE_READ_TIMEOUT: Duration = Duration::from_millis(1500);
/// 读取请求到达后再等一会儿，让目标软件读完并关闭剪贴板，还原时才打得开。
const PASTE_SETTLE: Duration = Duration::from_millis(100);
/// 写好剪贴板后稍等再按粘贴键，部分软件在剪贴板刚变化时还在处理通知。
const PASTE_KEY_DELAY: Duration = Duration::from_millis(60);

/// 这次粘贴交出的文字。按键之前的读取来自剪贴板监视程序，不算目标软件读过。
struct OfferedText {
    utf16: Vec<u16>,
    keys_sent: bool,
    read: bool,
}

thread_local! {
    static OFFERED: RefCell<Option<OfferedText>> = const { RefCell::new(None) };
}

/// 告诉剪贴板历史与剪贴板管理器不要记录这段临时文字（Windows 剪贴板历史、云剪贴板与
/// 遵守 `ExcludeClipboardContentFromMonitorProcessing` 约定的第三方工具）。
const TRANSIENT_MARKERS: [(windows::core::PCWSTR, &[u8]); 3] = [
    (w!("ExcludeClipboardContentFromMonitorProcessing"), &[0]),
    (w!("CanIncludeInClipboardHistory"), &[0, 0, 0, 0]),
    (w!("CanUploadToCloudClipboard"), &[0, 0, 0, 0]),
];

unsafe extern "system" fn paste_window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_RENDERFORMAT && wparam.0 == CF_UNICODETEXT.0 as usize {
        render_offered_text();
        return LRESULT(0);
    }
    if message == WM_RENDERALLFORMATS {
        // 粘贴窗口销毁时文字还没交出：放上剪贴板，免得留下读不出的格式。
        hand_over_offered_text(hwnd);
        return LRESULT(0);
    }
    DefWindowProcW(hwnd, message, wparam, lparam)
}

fn render_offered_text() {
    let utf16 = OFFERED.with_borrow_mut(|offered| {
        offered.as_mut().map(|offered| {
            offered.read |= offered.keys_sent;
            offered.utf16.clone()
        })
    });
    let Some(utf16) = utf16 else { return };
    let bytes: Vec<u8> = utf16.iter().flat_map(|unit| unit.to_ne_bytes()).collect();
    // 处理 WM_RENDERFORMAT 时剪贴板已由读取方打开，直接放入数据。
    if let Err(error) = set_clipboard_bytes(CF_UNICODETEXT.0.into(), &bytes) {
        crate::application::diagnostics::event(
            "warn",
            "clipboard.renderFailed",
            serde_json::json!({ "error": error }),
        );
    }
}

/// 把还没交出的文字真正放上剪贴板。粘贴线程不处理消息时，剪贴板上不能留着要等它交出数据的格式，
/// 否则其他程序一读剪贴板就卡住，读到的还是空的。
fn hand_over_offered_text(window: HWND) {
    let Ok(_clipboard) = ClipboardGuard::open(window) else { return };
    if unsafe { GetClipboardOwner() }.ok() == Some(window) {
        render_offered_text();
    }
}

fn offered_text_read() -> bool {
    OFFERED.with_borrow(|offered| offered.as_ref().is_some_and(|offered| offered.read))
}

fn set_clipboard_bytes(format: u32, bytes: &[u8]) -> Result<(), String> {
    unsafe {
        let global = GlobalAlloc(GMEM_MOVEABLE, bytes.len())
            .map_err(|error| format!("分配剪贴板内存失败：{error}"))?;
        let target = GlobalLock(global).cast::<u8>();
        if target.is_null() {
            let _ = GlobalFree(global);
            return Err("锁定剪贴板内存失败".into());
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), target, bytes.len());
        // 解锁到计数为零时也返回「失败」，错误码为成功，不必处理。
        let _ = GlobalUnlock(global);
        if let Err(error) = SetClipboardData(format, HANDLE(global.0)) {
            let _ = GlobalFree(global);
            return Err(format!("写入 Windows 剪贴板格式 {format} 失败：{error}"));
        }
    }
    Ok(())
}

/// 只接收剪贴板消息的隐藏窗口，属于粘贴线程。
fn create_paste_window() -> Result<HWND, String> {
    unsafe {
        let instance = GetModuleHandleW(None).map_err(|error| format!("创建粘贴窗口失败：{error}"))?;
        let class = w!("SayItClipboardPaste");
        let window_class = WNDCLASSW {
            lpfnWndProc: Some(paste_window_proc),
            hInstance: instance.into(),
            lpszClassName: class,
            ..Default::default()
        };
        if RegisterClassW(&window_class) == 0 {
            return Err(format!("创建粘贴窗口失败：{}", windows::core::Error::from_win32()));
        }
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class,
            w!(""),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            HMENU::default(),
            instance,
            None,
        )
        .map_err(|error| format!("创建粘贴窗口失败：{error}"))
    }
}

/// 处理本线程的消息（包括其他进程同步发来的读取请求），直到 `done` 成立或到期。
fn pump_messages_until(deadline: Instant, done: impl Fn() -> bool) {
    loop {
        let mut message = MSG::default();
        while unsafe { PeekMessageW(&mut message, HWND::default(), 0, 0, PM_REMOVE) }.as_bool() {
            unsafe { DispatchMessageW(&message) };
        }
        let now = Instant::now();
        if done() || now >= deadline {
            return;
        }
        let wait = (deadline - now).as_millis().try_into().unwrap_or(u32::MAX);
        unsafe { MsgWaitForMultipleObjects(None, BOOL(0), wait, QS_ALLINPUT) };
    }
}

struct PasteJob {
    text: String,
    pasted: std::sync::mpsc::Sender<Result<(), String>>,
}

pub(crate) fn paste_text(text: &str) -> Result<(), String> {
    static WORKER: std::sync::OnceLock<Result<std::sync::mpsc::Sender<PasteJob>, String>> =
        std::sync::OnceLock::new();
    let worker = WORKER
        .get_or_init(|| {
            let (tx, rx) = std::sync::mpsc::channel::<PasteJob>();
            std::thread::Builder::new()
                .name("clipboard-paste".into())
                .spawn(move || run_paste_worker(rx))
                .map(|_| tx)
                .map_err(|error| format!("创建粘贴线程失败：{error}"))
        })
        .as_ref()
        .map_err(Clone::clone)?;
    let (pasted, result) = std::sync::mpsc::channel();
    worker
        .send(PasteJob { text: text.to_string(), pasted })
        .map_err(|_| "粘贴线程已退出".to_string())?;
    result.recv().map_err(|_| "粘贴线程已退出".to_string())?
}

fn run_paste_worker(jobs: std::sync::mpsc::Receiver<PasteJob>) {
    let window = create_paste_window();
    for job in jobs {
        let _restoring = CLIPBOARD_RESTORE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        // 每次粘贴各自初始化、还原后释放 OLE，不常驻单线程套间。
        let ole = unsafe { OleInitialize(None) }
            .map(|()| OleGuard)
            .map_err(|error| format!("初始化 Windows OLE 剪贴板失败：{error}"));
        let pasted = match (&ole, &window) {
            (Ok(_), Ok(window)) => paste_once(*window, &job.text).map(|backup| (*window, backup)),
            (Err(error), _) | (_, Err(error)) => Err(error.clone()),
        };
        let (result, pending_restore) = match pasted {
            Ok(pending) => (Ok(()), Some(pending)),
            Err(error) => (Err(error), None),
        };
        let _ = job.pasted.send(result);
        if let Some((window, backup)) = pending_restore {
            pump_messages_until(Instant::now() + PASTE_READ_TIMEOUT, offered_text_read);
            let read = offered_text_read();
            if read {
                pump_messages_until(Instant::now() + PASTE_SETTLE, || false);
            } else {
                // 一直没等到读取：可能是剪贴板监视程序在按键前读走了数据，真机上据此判断出现频率。
                crate::application::diagnostics::event(
                    "info",
                    "clipboard.pasteReadTimeout",
                    serde_json::json!({}),
                );
            }
            if let Err(error) = restore_if_owned(backup, window) {
                hand_over_offered_text(window);
                crate::application::diagnostics::event(
                    "warn",
                    "clipboard.restoreFailed",
                    serde_json::json!({ "error": error }),
                );
            }
        }
        OFFERED.set(None);
    }
}

/// 写入文本并按下粘贴键。成功时返回还原所需的备份，由调用方等目标软件读完后还原；
/// 写入或按键失败时当场还原并返回错误。
fn paste_once(window: HWND, text: &str) -> Result<ClipboardSnapshot, String> {
    let backup = ClipboardSnapshot::capture()?;
    let pasted = offer_text(window, text).and_then(|()| {
        // 等待期间照常处理消息：剪贴板监视程序这时来读，不能让它卡住。
        pump_messages_until(Instant::now() + PASTE_KEY_DELAY, || false);
        OFFERED.with_borrow_mut(|offered| {
            if let Some(offered) = offered {
                offered.keys_sent = true;
            }
        });
        press_paste_keys()
    });
    match pasted {
        Ok(()) => Ok(backup),
        Err(error) => Err(match restore_if_owned(backup, window) {
            Ok(()) => error,
            Err(restore) => {
                hand_over_offered_text(window);
                format!("{error}；{restore}")
            }
        }),
    }
}

fn press_paste_keys() -> Result<(), String> {
    let mut enigo =
        Enigo::new(&EnigoSettings::default()).map_err(|error| format!("初始化输入失败：{error}"))?;
    enigo
        .key(Key::Control, Direction::Press)
        .map_err(|error| format!("模拟粘贴失败：{error}"))?;
    let click = enigo
        .key(Key::Unicode('v'), Direction::Click)
        .map_err(|error| format!("模拟粘贴失败：{error}"));
    let release = enigo
        .key(Key::Control, Direction::Release)
        .map_err(|error| format!("释放粘贴按键失败：{error}"));
    click.and(release)
}

/// 剪贴板上只登记文字格式，数据等读取时再交出；临时标记直接写入。
fn offer_text(window: HWND, text: &str) -> Result<(), String> {
    let utf16 = text.encode_utf16().chain(std::iter::once(0)).collect();
    OFFERED.set(Some(OfferedText { utf16, keys_sent: false, read: false }));
    let _clipboard = ClipboardGuard::open(window)?;
    unsafe {
        EmptyClipboard().map_err(|error| format!("清空 Windows 剪贴板失败：{error}"))?;
        // 延迟渲染成功时返回空句柄，windows 库会把它当作失败，所以按错误码判断。
        SetLastError(WIN32_ERROR(0));
        if let Err(error) = SetClipboardData(CF_UNICODETEXT.0.into(), HANDLE::default()) {
            if error.code().is_err() {
                return Err(format!("写入 Windows 剪贴板失败：{error}"));
            }
        }
        for (name, data) in TRANSIENT_MARKERS {
            set_clipboard_bytes(RegisterClipboardFormatW(name), data)?;
        }
    }
    Ok(())
}

/// 粘贴窗口不再是剪贴板所有者，说明之后有程序写过剪贴板，不还原，以免覆盖用户新复制的内容。
fn restore_if_owned(backup: ClipboardSnapshot, window: HWND) -> Result<(), String> {
    if unsafe { GetClipboardOwner() }.ok() != Some(window) {
        return Ok(());
    }
    backup.restore()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 单个格式复制不了时，备份不得整份放弃。
    ///
    /// 以前 `capture()` 对 GetClipboardData 与 duplicate 都用 `?`，而 `paste_text` 一开头就是
    /// `ClipboardSnapshot::capture()?`：剪贴板里只要有一种拿不到的格式（延迟渲染、源进程
    /// 已退出的 GDI 对象等），听写结果就完全无法注入。
    #[test]
    fn a_single_unreadable_format_does_not_abandon_the_whole_backup() {
        let kept = duplicate_available(&[1, 13, 49161, 2], |format| {
            if format == 49161 {
                return Err("延迟渲染的格式拿不到".to_string());
            }
            Ok(format)
        });
        assert_eq!(kept, vec![1, 13, 2]);
    }

    /// 还原也不得中途放弃：半途返回会把剩下的格式丢掉，用户剪贴板缺得更多。
    #[test]
    fn restore_puts_back_every_format_it_can_and_still_reports_the_failure() {
        let mut formats = vec![1u32, 13, 2];
        let mut restored = Vec::new();
        let result = transfer_all(&mut formats, |format| {
            if *format == 13 {
                return Err("还原格式 13 失败".to_string());
            }
            restored.push(*format);
            Ok(())
        });
        assert_eq!(restored, vec![1, 2]);
        assert!(result.is_err(), "失败仍然要报出来");
    }

    #[test]
    fn clipboard_formats_use_matching_storage_medium() {
        assert_eq!(
            ClipboardMedium::for_format(CF_METAFILEPICT.0.into()),
            ClipboardMedium::MetafilePicture
        );
        assert_eq!(
            ClipboardMedium::for_format(CF_ENHMETAFILE.0.into()),
            ClipboardMedium::EnhancedMetafile
        );
        assert_eq!(
            ClipboardMedium::for_format(CF_BITMAP.0.into()),
            ClipboardMedium::Gdi
        );
        assert_eq!(
            ClipboardMedium::for_format(CF_GDIOBJFIRST.0.into()),
            ClipboardMedium::Gdi
        );
        assert_eq!(ClipboardMedium::for_format(13), ClipboardMedium::Global);
    }
}
