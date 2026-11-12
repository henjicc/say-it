//! 原生窗口的平台分发：命令投递、动作回传、环境开关与外观。
//!
//! 一个窗口只需在主 crate 实现 `OverlaySpec`，再经这里的 `post` 投递命令。

use sayit_overlay_core::spec::OverlaySpec;

const PLATFORM_DEFAULT: bool = cfg!(any(windows, target_os = "macos"));

/// SAYIT_NATIVE_<NAME>："0" 关闭，其它已设置值开启，未设置取平台默认。
pub(crate) fn native_window_enabled(var: &str) -> bool {
    let value = std::env::var_os(var);
    parse_switch(
        value.as_ref().map(|value| value.to_str().unwrap_or("")),
        PLATFORM_DEFAULT,
    )
}

fn parse_switch(value: Option<&str>, platform_default: bool) -> bool {
    value.map_or(platform_default, |value| value != "0")
}

/// 可在任意线程调用；第一条命令创建窗口。
pub(crate) fn post<S: OverlaySpec>(command: S::Command) {
    #[cfg(windows)]
    super::native_win::host::post::<S>(command);
    #[cfg(target_os = "macos")]
    sayit_overlay_macos::post::<S>(command);
    #[cfg(not(any(windows, target_os = "macos")))]
    let _ = command;
}

/// 只在窗口已经创建时投递，不因为这条命令创建窗口。
pub(crate) fn post_if_started<S: OverlaySpec>(command: S::Command) {
    #[cfg(windows)]
    super::native_win::host::post_if_started::<S>(command);
    #[cfg(target_os = "macos")]
    sayit_overlay_macos::post_if_started::<S>(command);
    #[cfg(not(any(windows, target_os = "macos")))]
    let _ = command;
}

pub(crate) fn started<S: OverlaySpec>() -> bool {
    #[cfg(windows)]
    {
        super::native_win::host::started::<S>()
    }
    #[cfg(target_os = "macos")]
    {
        sayit_overlay_macos::started::<S>()
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        false
    }
}

/// 应用启动时调用：macOS 提前采集屏幕信息，之后创建的窗口一开始就按所在屏的缩放建模型。
pub(crate) fn native_overlay_start() {
    #[cfg(target_os = "macos")]
    sayit_overlay_macos::start();
}

/// 供 `OverlaySpec::dispatch` 使用。macOS 的动作出自主线程或渲染线程，转到异步运行时执行；
/// Windows 的动作出自窗口自己的线程，直接执行。
pub(crate) fn run_action(action: impl FnOnce() + Send + 'static) {
    #[cfg(target_os = "macos")]
    tauri::async_runtime::spawn(async move { action() });
    #[cfg(not(target_os = "macos"))]
    action();
}

/// Windows 原生窗口固定使用 WindowsLegacy；Mac 各窗共用可变外观。
pub(crate) fn native_overlay_set_look(look: sayit_overlay_core::look::Look) {
    #[cfg(target_os = "macos")]
    sayit_overlay_macos::set_look(look);
    #[cfg(not(target_os = "macos"))]
    let _ = look;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unset_switch_uses_platform_default() {
        assert!(parse_switch(None, true));
        assert!(!parse_switch(None, false));
    }

    #[test]
    fn only_exact_zero_disables_an_explicit_switch() {
        for default in [true, false] {
            assert!(!parse_switch(Some("0"), default));
            for value in ["", "1", "false", "off", "00", " 0", "0 "] {
                assert!(parse_switch(Some(value), default), "{value:?}");
            }
        }
    }
}
