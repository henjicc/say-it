use enigo::{Direction, Enigo, Key, Keyboard, Settings as EnigoSettings};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Graphics::Gdi::{HBITMAP, HENHMETAFILE};
use windows::Win32::System::Com::{
    STGMEDIUM, STGMEDIUM_0, TYMED_ENHMF, TYMED_GDI, TYMED_HGLOBAL, TYMED_MFPICT,
};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData,
    GetClipboardSequenceNumber, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::GMEM_MOVEABLE;
use windows::Win32::System::Ole::{
    OleDuplicateData, OleInitialize, OleUninitialize, ReleaseStgMedium, CF_BITMAP, CF_DSPBITMAP,
    CF_DSPENHMETAFILE, CF_DSPMETAFILEPICT, CF_ENHMETAFILE, CF_GDIOBJFIRST, CF_GDIOBJLAST,
    CF_METAFILEPICT, CF_PALETTE, CLIPBOARD_FORMAT,
};

const CLIPBOARD_OPEN_ATTEMPTS: usize = 10;
const CLIPBOARD_OPEN_RETRY_DELAY: std::time::Duration = std::time::Duration::from_millis(5);

struct OleGuard;
impl Drop for OleGuard {
    fn drop(&mut self) {
        unsafe { OleUninitialize() }
    }
}

struct ClipboardGuard;

impl ClipboardGuard {
    fn open() -> Result<Self, String> {
        let mut last_error = None;
        for _ in 0..CLIPBOARD_OPEN_ATTEMPTS {
            match unsafe { OpenClipboard(None) } {
                Ok(()) => return Ok(Self),
                Err(error) => {
                    last_error = Some(error);
                    std::thread::sleep(CLIPBOARD_OPEN_RETRY_DELAY);
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
        let _clipboard = ClipboardGuard::open()?;
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
        let _clipboard = ClipboardGuard::open()?;
        unsafe { EmptyClipboard() }
            .map_err(|error| format!("清空临时 Windows 剪贴板失败：{error}"))?;
        transfer_all(&mut self.formats, |format| format.transfer_to_clipboard())
    }
}

/// 粘贴后要等目标软件读完剪贴板（约 180ms）才能还原。这段等待和还原放在专用线程上做，
/// 调用方在按下粘贴键后就返回，胶囊与结束音不必再等它。所有粘贴在这条线程上排队，
/// 下一次粘贴备份剪贴板时，上一次的还原一定已经做完。
///
/// 粘贴线程从备份到还原完成一直持有这把锁；其他要读写剪贴板的地方先调用
/// `wait_clipboard_restored`，免得读到注入的文字，或被随后的还原覆盖。
static CLIPBOARD_RESTORE: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub(crate) fn wait_clipboard_restored() {
    drop(CLIPBOARD_RESTORE.lock().unwrap_or_else(|poisoned| poisoned.into_inner()));
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
    for job in jobs {
        let _restoring = CLIPBOARD_RESTORE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        // 每次粘贴各自初始化、还原后释放 OLE：线程在两次粘贴之间不处理窗口消息，不常驻单线程套间。
        let ole = unsafe { OleInitialize(None) }
            .map(|()| OleGuard)
            .map_err(|error| format!("初始化 Windows OLE 剪贴板失败：{error}"));
        let pending_restore = match &ole {
            Ok(_) => paste_once(&job.text),
            Err(error) => Err(error.clone()),
        };
        let (result, pending_restore) = match pending_restore {
            Ok(pending) => (Ok(()), Some(pending)),
            Err(error) => (Err(error), None),
        };
        let _ = job.pasted.send(result);
        if let Some((backup, clipboard, injected_sequence)) = pending_restore {
            std::thread::sleep(std::time::Duration::from_millis(180));
            if let Err(error) = restore_if_unchanged(backup, clipboard, injected_sequence) {
                crate::application::diagnostics::event(
                    "warn",
                    "clipboard.restoreFailed",
                    serde_json::json!({ "error": error }),
                );
            }
        }
    }
}

/// 写入文本并按下粘贴键。成功时返回还原所需的备份，由调用方等目标软件读完后还原；
/// 按键失败时当场还原并返回错误。
fn paste_once(text: &str) -> Result<(ClipboardSnapshot, arboard::Clipboard, u32), String> {
    let backup = ClipboardSnapshot::capture()?;
    let mut clipboard =
        arboard::Clipboard::new().map_err(|error| format!("打开剪贴板失败：{error}"))?;
    clipboard
        .set_text(text.to_string())
        .map_err(|error| format!("写入剪贴板失败：{error}"))?;
    let injected_sequence = unsafe { GetClipboardSequenceNumber() };
    std::thread::sleep(std::time::Duration::from_millis(60));
    let mut enigo = Enigo::new(&EnigoSettings::default())
        .map_err(|error| format!("初始化输入失败：{error}"))?;
    enigo
        .key(Key::Control, Direction::Press)
        .map_err(|error| format!("模拟粘贴失败：{error}"))?;
    let click = enigo
        .key(Key::Unicode('v'), Direction::Click)
        .map_err(|error| format!("模拟粘贴失败：{error}"));
    let release = enigo
        .key(Key::Control, Direction::Release)
        .map_err(|error| format!("释放粘贴按键失败：{error}"));
    match click.and(release) {
        Ok(()) => Ok((backup, clipboard, injected_sequence)),
        Err(error) => Err(match restore_if_unchanged(backup, clipboard, injected_sequence) {
            Ok(()) => error,
            Err(restore) => format!("{error}；{restore}"),
        }),
    }
}

/// 剪贴板在粘贴之后被别的程序改过时不还原，以免覆盖用户新复制的内容。
fn restore_if_unchanged(
    backup: ClipboardSnapshot,
    clipboard: arboard::Clipboard,
    injected_sequence: u32,
) -> Result<(), String> {
    if unsafe { GetClipboardSequenceNumber() } != injected_sequence {
        return Ok(());
    }
    drop(clipboard);
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
