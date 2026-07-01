/// 注册表字体值名形如 "Microsoft YaHei & Microsoft YaHei UI (TrueType)"，
/// 需要去掉尾部的格式标注、按 "&" 拆开同一条目里捆绑的多个字重/字体族名。
fn parse_font_family_names(value_name: &str) -> Vec<String> {
    let without_suffix = value_name
        .rsplit_once(" (")
        .map(|(name, _)| name)
        .unwrap_or(value_name);
    without_suffix
        .split('&')
        .map(|part| part.trim().to_string())
        .filter(|name| !name.is_empty())
        .collect()
}

#[cfg(windows)]
fn read_fonts_key(hive: winreg::RegKey) -> Vec<String> {
    let Ok(fonts_key) = hive.open_subkey("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Fonts")
    else {
        return Vec::new();
    };
    fonts_key
        .enum_values()
        .filter_map(|entry| entry.ok())
        .flat_map(|(name, _)| parse_font_family_names(&name))
        .collect()
}

#[cfg(windows)]
#[tauri::command]
pub(crate) fn list_system_fonts() -> Result<Vec<String>, String> {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    use winreg::RegKey;

    let mut names: Vec<String> = Vec::new();
    names.extend(read_fonts_key(RegKey::predef(HKEY_LOCAL_MACHINE)));
    names.extend(read_fonts_key(RegKey::predef(HKEY_CURRENT_USER)));

    names.sort();
    names.dedup();
    Ok(names)
}

#[cfg(not(windows))]
#[tauri::command]
pub(crate) fn list_system_fonts() -> Result<Vec<String>, String> {
    Ok(Vec::new())
}
