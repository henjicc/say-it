use serde_json::Value;

use super::plugin::PluginRuntimeSpec;

const KEYCHAIN_SERVICE: &str = "com.sayit.provider-plugin-session";
const LEGACY_SESSION_FILE: &str = "session.dpapi";

pub fn load_session(spec: &PluginRuntimeSpec) -> Result<Value, String> {
    let entry = keyring::Entry::new(KEYCHAIN_SERVICE, &spec.plugin_id)
        .map_err(|error| format!("打开系统凭据存储失败：{error}"))?;
    match entry.get_password() {
        Ok(value) => {
            serde_json::from_str(&value).map_err(|error| format!("插件会话数据损坏：{error}"))
        }
        Err(keyring::Error::NoEntry) => migrate_legacy_session(spec),
        Err(error) => Err(format!("读取系统凭据存储失败：{error}")),
    }
}

pub fn save_session(spec: &PluginRuntimeSpec, session: &Value) -> Result<(), String> {
    let entry = keyring::Entry::new(KEYCHAIN_SERVICE, &spec.plugin_id)
        .map_err(|error| format!("打开系统凭据存储失败：{error}"))?;
    let value = serde_json::to_string(session).map_err(|error| error.to_string())?;
    entry
        .set_password(&value)
        .map_err(|error| format!("写入系统凭据存储失败：{error}"))
}

pub fn clear_session(spec: &PluginRuntimeSpec) -> Result<(), String> {
    let entry = keyring::Entry::new(KEYCHAIN_SERVICE, &spec.plugin_id)
        .map_err(|error| format!("打开系统凭据存储失败：{error}"))?;
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => {}
        Err(error) => return Err(format!("清除系统凭据存储失败：{error}")),
    }
    let legacy = spec.data_dir.join(LEGACY_SESSION_FILE);
    if legacy.exists() {
        std::fs::remove_file(legacy).map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[cfg(windows)]
fn migrate_legacy_session(spec: &PluginRuntimeSpec) -> Result<Value, String> {
    let path = spec.data_dir.join(LEGACY_SESSION_FILE);
    if !path.exists() {
        return Ok(Value::Null);
    }
    let encrypted = std::fs::read(&path).map_err(|error| error.to_string())?;
    let plain = unprotect(&encrypted)?;
    let session: Value =
        serde_json::from_slice(&plain).map_err(|error| format!("旧版插件会话数据损坏：{error}"))?;
    save_session(spec, &session)?;
    std::fs::remove_file(path).map_err(|error| error.to_string())?;
    Ok(session)
}

#[cfg(not(windows))]
fn migrate_legacy_session(_spec: &PluginRuntimeSpec) -> Result<Value, String> {
    Ok(Value::Null)
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

#[cfg(all(test, windows))]
mod tests {
    use super::*;

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

    #[test]
    fn legacy_dpapi_data_can_be_migrated() {
        let plain = br#"{"cookie":"secret"}"#;
        assert_eq!(unprotect(&protect(plain).unwrap()).unwrap(), plain);
    }
}
