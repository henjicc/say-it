use enigo::{Direction, Enigo, Key, Keyboard, Settings as EnigoSettings};
use windows::Win32::System::DataExchange::GetClipboardSequenceNumber;
use windows::Win32::System::Ole::{
    OleFlushClipboard, OleGetClipboard, OleInitialize, OleSetClipboard, OleUninitialize,
};

struct OleGuard;
impl Drop for OleGuard {
    fn drop(&mut self) {
        unsafe { OleUninitialize() }
    }
}

pub(crate) fn paste_text(text: &str) -> Result<(), String> {
    unsafe { OleInitialize(None) }
        .map_err(|error| format!("初始化 Windows OLE 剪贴板失败：{error}"))?;
    let _guard = OleGuard;
    let backup = unsafe { OleGetClipboard() }.ok();
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
    click?;
    release?;
    std::thread::sleep(std::time::Duration::from_millis(180));
    if unsafe { GetClipboardSequenceNumber() } == injected_sequence {
        if let Some(backup) = backup {
            unsafe { OleSetClipboard(&backup) }
                .map_err(|error| format!("恢复 Windows 剪贴板失败：{error}"))?;
            unsafe { OleFlushClipboard() }
                .map_err(|error| format!("提交 Windows 剪贴板恢复失败：{error}"))?;
        } else {
            let _ = clipboard.clear();
        }
    }
    Ok(())
}
