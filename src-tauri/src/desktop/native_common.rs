//! 原生悬浮窗共用的环境开关与外观分发。

const PLATFORM_DEFAULT: bool = cfg!(windows);

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

/// Windows 原生窗口固定使用 WindowsLegacy；Mac 三窗共用可变外观。
pub(crate) fn native_overlay_set_look(look: sayit_overlay_core::look::Look) {
    #[cfg(target_os = "macos")]
    super::native_mac::set_look(look);
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
