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

impl ClipboardSnapshot {
    fn capture() -> Result<Self, String> {
        let _clipboard = ClipboardGuard::open()?;
        let mut formats = Vec::new();
        let mut format = 0;
        loop {
            format = unsafe { EnumClipboardFormats(format) };
            if format == 0 {
                break;
            }
            let source = unsafe { GetClipboardData(format) }
                .map_err(|error| format!("读取 Windows 剪贴板格式 {format} 失败：{error}"))?;
            formats.push(OwnedClipboardFormat::duplicate(format, source)?);
        }
        Ok(Self { formats })
    }

    fn restore(mut self) -> Result<(), String> {
        let _clipboard = ClipboardGuard::open()?;
        unsafe { EmptyClipboard() }
            .map_err(|error| format!("清空临时 Windows 剪贴板失败：{error}"))?;
        for format in &mut self.formats {
            format.transfer_to_clipboard()?;
        }
        Ok(())
    }
}

pub(crate) fn paste_text(text: &str) -> Result<(), String> {
    unsafe { OleInitialize(None) }
        .map_err(|error| format!("初始化 Windows OLE 剪贴板失败：{error}"))?;
    let _guard = OleGuard;
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
    let paste_result = click.and(release);
    if paste_result.is_ok() {
        std::thread::sleep(std::time::Duration::from_millis(180));
    }
    let restore_result = if unsafe { GetClipboardSequenceNumber() } == injected_sequence {
        drop(clipboard);
        backup.restore()
    } else {
        Ok(())
    };
    paste_result.and(restore_result)
}

#[cfg(test)]
mod tests {
    use super::*;

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
