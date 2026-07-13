use std::path::Path;

use serde_json::Value;

use super::plugin::PluginProcessSpec;

const SESSION_FILE: &str = "session.dpapi";

pub fn load_session(spec: &PluginProcessSpec) -> Result<Value, String> {
    let path = spec.data_dir.join(SESSION_FILE);
    if !path.exists() {
        return Ok(Value::Null);
    }
    let encrypted = std::fs::read(&path).map_err(|error| error.to_string())?;
    let plain = unprotect(&encrypted)?;
    serde_json::from_slice(&plain).map_err(|error| format!("插件会话数据损坏：{error}"))
}

pub fn save_session(spec: &PluginProcessSpec, session: &Value) -> Result<(), String> {
    std::fs::create_dir_all(&spec.data_dir).map_err(|error| error.to_string())?;
    let plain = serde_json::to_vec(session).map_err(|error| error.to_string())?;
    let encrypted = protect(&plain)?;
    let path = spec.data_dir.join(SESSION_FILE);
    let temporary = path.with_extension("tmp");
    std::fs::write(&temporary, encrypted).map_err(|error| error.to_string())?;
    replace_file(&temporary, &path)
}

pub fn clear_session(spec: &PluginProcessSpec) -> Result<(), String> {
    let path = spec.data_dir.join(SESSION_FILE);
    if path.exists() {
        std::fs::remove_file(path).map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn replace_file(source: &Path, target: &Path) -> Result<(), String> {
    if target.exists() {
        std::fs::remove_file(target).map_err(|error| error.to_string())?;
    }
    std::fs::rename(source, target).map_err(|error| error.to_string())
}

#[cfg(windows)]
fn protect(data: &[u8]) -> Result<Vec<u8>, String> {
    use windows::core::w;
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::Security::Cryptography::{
        CryptProtectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    let input = CRYPT_INTEGER_BLOB {
        cbData: data.len().try_into().map_err(|_| "会话数据过大")?,
        pbData: data.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    unsafe {
        CryptProtectData(
            &input,
            w!("SayIt provider plugin session"),
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
        .map_err(|error| error.to_string())?;
        let bytes = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        let _ = LocalFree(HLOCAL(output.pbData.cast()));
        Ok(bytes)
    }
}

#[cfg(windows)]
fn unprotect(data: &[u8]) -> Result<Vec<u8>, String> {
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::Security::Cryptography::{
        CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    let input = CRYPT_INTEGER_BLOB {
        cbData: data.len().try_into().map_err(|_| "会话数据过大")?,
        pbData: data.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    unsafe {
        CryptUnprotectData(
            &input,
            None,
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
        .map_err(|error| error.to_string())?;
        let bytes = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        let _ = LocalFree(HLOCAL(output.pbData.cast()));
        Ok(bytes)
    }
}

#[cfg(not(windows))]
fn protect(_data: &[u8]) -> Result<Vec<u8>, String> {
    Err("受保护插件会话目前仅支持 Windows".into())
}

#[cfg(not(windows))]
fn unprotect(_data: &[u8]) -> Result<Vec<u8>, String> {
    Err("受保护插件会话目前仅支持 Windows".into())
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn dpapi_round_trip_uses_current_windows_user() {
        let plain = br#"{"cookie":"secret"}"#;
        assert_eq!(unprotect(&protect(plain).unwrap()).unwrap(), plain);
    }
}
