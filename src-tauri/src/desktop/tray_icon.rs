//! 状态栏 / 托盘图标：只画「声推成字」主体，不带应用图标的蓝色底，与系统和其他应用的单色图标一致。
//! 源文件在 `icons/source/tray-*.svg`，用 `tauri icon -p` 渲染成 `icons/tray/` 下的 PNG。
use tauri::image::Image;
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::Runtime;

/// macOS 用模板图：系统按菜单栏深浅自动画成黑或白。图宽高比 22:18，系统把高度缩到 18pt。
#[cfg(target_os = "macos")]
pub(crate) fn apply<R: Runtime>(builder: TrayIconBuilder<R>) -> tauri::Result<TrayIconBuilder<R>> {
    Ok(builder
        .icon(Image::from_bytes(include_bytes!("../../icons/tray/macos.png"))?)
        .icon_as_template(true))
}

/// Windows 托盘没有模板图，按任务栏深浅选白色或黑色版本。
#[cfg(windows)]
pub(crate) fn apply<R: Runtime>(builder: TrayIconBuilder<R>) -> tauri::Result<TrayIconBuilder<R>> {
    let light = taskbar_is_light();
    SHOWN_LIGHT.store(light, std::sync::atomic::Ordering::Relaxed);
    Ok(builder.icon(windows_icon(light)?))
}

/// 当前托盘显示的是不是浅色任务栏版本。
#[cfg(windows)]
static SHOWN_LIGHT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 用户切换了任务栏深浅后，指针移到托盘图标上时换成对应版本。
#[cfg(windows)]
pub(crate) fn refresh<R: Runtime>(tray: &TrayIcon<R>) {
    let light = taskbar_is_light();
    if SHOWN_LIGHT.swap(light, std::sync::atomic::Ordering::Relaxed) == light {
        return;
    }
    if let Err(error) = windows_icon(light).and_then(|icon| tray.set_icon(Some(icon))) {
        eprintln!("[tray] 切换托盘图标失败: {error}");
    }
}

#[cfg(not(windows))]
pub(crate) fn refresh<R: Runtime>(_tray: &TrayIcon<R>) {}

#[cfg(windows)]
fn windows_icon(light_taskbar: bool) -> tauri::Result<Image<'static>> {
    let bytes: &[u8] = if light_taskbar {
        include_bytes!("../../icons/tray/windows-light.png")
    } else {
        include_bytes!("../../icons/tray/windows-dark.png")
    };
    Image::from_bytes(bytes)
}

/// 任务栏深浅由 `SystemUsesLightTheme` 决定（与应用的 `AppsUseLightTheme` 分开设置）；读不到按深色处理，
/// 这是 Windows 11 的默认值。
#[cfg(windows)]
fn taskbar_is_light() -> bool {
    winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER)
        .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize")
        .and_then(|key| key.get_value::<u32, _>("SystemUsesLightTheme"))
        .is_ok_and(|value| value != 0)
}

#[cfg(not(any(windows, target_os = "macos")))]
pub(crate) fn apply<R: Runtime>(builder: TrayIconBuilder<R>) -> tauri::Result<TrayIconBuilder<R>> {
    Ok(builder)
}
