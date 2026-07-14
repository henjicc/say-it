use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::plugin::PluginRuntimeSpec;

const KEYCHAIN_SERVICE: &str = "com.sayit.provider-plugin-session";
const LEGACY_SESSION_FILE: &str = "session.dpapi";
// Windows Credential Manager 的凭据 Blob 上限为 2560 字节；password 以 UTF-16 编码。
// 1200 个 UTF-16 代码单元可为实现细节保留余量，并避免截断非 BMP 字符。
const KEYCHAIN_CHUNK_UTF16_LIMIT: usize = 1_200;
const MAX_SESSION_CHUNKS: usize = 512;
static SESSION_STORAGE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ChunkedSessionReference {
    format: String,
    storage_key: String,
    chunks: usize,
}

pub fn load_session(spec: &PluginRuntimeSpec) -> Result<Value, String> {
    let entry = session_entry(spec)?;
    match entry.get_password() {
        Ok(value) => load_stored_session(spec, &value),
        Err(keyring::Error::NoEntry) => migrate_legacy_session(spec),
        Err(error) => Err(format!("读取系统凭据存储失败：{error}")),
    }
}

pub fn save_session(spec: &PluginRuntimeSpec, session: &Value) -> Result<(), String> {
    let value = serde_json::to_string(session).map_err(|error| error.to_string())?;
    let chunks = split_for_keychain(&value);
    if chunks.len() > MAX_SESSION_CHUNKS {
        return Err("登录会话过大，无法安全保存到系统凭据存储".into());
    }

    let entry = session_entry(spec)?;
    let previous = match entry.get_password() {
        Ok(value) => parse_chunked_reference(&value),
        Err(keyring::Error::NoEntry) => None,
        Err(error) => return Err(format!("读取当前登录会话失败：{error}")),
    };
    let reference = ChunkedSessionReference {
        format: "chunked-v1".into(),
        storage_key: next_storage_key(),
        chunks: chunks.len(),
    };

    let mut stored = 0;
    for (index, chunk) in chunks.iter().enumerate() {
        if let Err(error) = session_chunk_entry(spec, &reference, index)?.set_password(chunk) {
            delete_chunks(spec, &reference, stored);
            return Err(format!("写入系统凭据存储失败：{error}"));
        }
        stored += 1;
    }

    let descriptor = serde_json::to_string(&reference).map_err(|error| error.to_string())?;
    if let Err(error) = entry.set_password(&descriptor) {
        delete_chunks(spec, &reference, stored);
        return Err(format!("写入系统凭据存储失败：{error}"));
    }
    if let Some(previous) = previous {
        delete_chunks(spec, &previous, previous.chunks);
    }
    Ok(())
}

pub fn clear_session(spec: &PluginRuntimeSpec) -> Result<(), String> {
    let entry = session_entry(spec)?;
    let reference = match entry.get_password() {
        Ok(value) => parse_chunked_reference(&value),
        Err(keyring::Error::NoEntry) => None,
        Err(error) => return Err(format!("读取当前登录会话失败：{error}")),
    };
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => {}
        Err(error) => return Err(format!("清除系统凭据存储失败：{error}")),
    }
    if let Some(reference) = reference {
        delete_chunks(spec, &reference, reference.chunks);
    }
    let legacy = spec.data_dir.join(LEGACY_SESSION_FILE);
    if legacy.exists() {
        std::fs::remove_file(legacy).map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn session_entry(spec: &PluginRuntimeSpec) -> Result<keyring::Entry, String> {
    keyring::Entry::new(KEYCHAIN_SERVICE, &spec.plugin_id)
        .map_err(|error| format!("打开系统凭据存储失败：{error}"))
}

fn session_chunk_entry(
    spec: &PluginRuntimeSpec,
    reference: &ChunkedSessionReference,
    index: usize,
) -> Result<keyring::Entry, String> {
    keyring::Entry::new(
        KEYCHAIN_SERVICE,
        &format!(
            "{}:session:{}:{index}",
            spec.plugin_id, reference.storage_key
        ),
    )
    .map_err(|error| format!("打开系统凭据存储失败：{error}"))
}

fn load_stored_session(spec: &PluginRuntimeSpec, value: &str) -> Result<Value, String> {
    let value = if let Some(reference) = parse_chunked_reference(value) {
        let mut content = String::new();
        for index in 0..reference.chunks {
            let chunk = session_chunk_entry(spec, &reference, index)?
                .get_password()
                .map_err(|error| format!("读取登录会话分片失败：{error}"))?;
            content.push_str(&chunk);
        }
        content
    } else {
        value.to_string()
    };
    serde_json::from_str(&value).map_err(|error| format!("插件会话数据损坏：{error}"))
}

fn parse_chunked_reference(value: &str) -> Option<ChunkedSessionReference> {
    let reference = serde_json::from_str::<ChunkedSessionReference>(value).ok()?;
    (reference.format == "chunked-v1"
        && !reference.storage_key.is_empty()
        && (1..=MAX_SESSION_CHUNKS).contains(&reference.chunks))
    .then_some(reference)
}

fn split_for_keychain(value: &str) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut current_units = 0;
    for character in value.chars() {
        let units = character.len_utf16();
        if current_units + units > KEYCHAIN_CHUNK_UTF16_LIMIT && !current.is_empty() {
            chunks.push(current);
            current = String::new();
            current_units = 0;
        }
        current.push(character);
        current_units += units;
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

fn next_storage_key() -> String {
    let sequence = SESSION_STORAGE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    format!("{nanos:x}-{sequence:x}")
}

fn delete_chunks(spec: &PluginRuntimeSpec, reference: &ChunkedSessionReference, count: usize) {
    for index in 0..count {
        if let Ok(entry) = session_chunk_entry(spec, reference, index) {
            if let Err(error) = entry.delete_credential() {
                if !matches!(error, keyring::Error::NoEntry) {
                    crate::dlog!("[plugin] 清理过期登录会话分片失败：{error}");
                }
            }
        }
    }
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

    fn test_spec() -> PluginRuntimeSpec {
        let nonce = next_storage_key();
        PluginRuntimeSpec {
            plugin_id: format!("sayit-session-test-{nonce}"),
            root: std::path::PathBuf::new(),
            entrypoint: std::path::PathBuf::new(),
            permissions: Vec::new(),
            allowed_hosts: Vec::new(),
            data_dir: std::env::temp_dir().join(format!("sayit-session-test-{nonce}")),
            trust: "trusted".into(),
        }
    }

    #[test]
    fn session_chunks_keep_utf16_boundary_and_original_content() {
        let value = format!(
            "{}😀{}",
            "a".repeat(KEYCHAIN_CHUNK_UTF16_LIMIT - 1),
            "b".repeat(10)
        );
        let chunks = split_for_keychain(&value);
        assert!(chunks
            .iter()
            .all(|chunk| chunk.encode_utf16().count() <= KEYCHAIN_CHUNK_UTF16_LIMIT));
        assert_eq!(chunks.concat(), value);
    }

    #[test]
    fn only_explicit_chunked_reference_is_recognized() {
        assert!(parse_chunked_reference(
            r#"{"format":"chunked-v1","storageKey":"key","chunks":2}"#
        )
        .is_some());
        assert!(parse_chunked_reference(
            r#"{"format":"chunked-v1","storageKey":"key","chunks":0}"#
        )
        .is_none());
        assert!(parse_chunked_reference(r#"{"cookies":[]}"#).is_none());
    }

    #[test]
    fn long_session_round_trips_through_system_credentials() {
        let spec = test_spec();
        let session = serde_json::json!({
            "cookies": [{ "name": "session", "value": "x".repeat(6_000) }]
        });
        save_session(&spec, &session).unwrap();
        assert!(
            parse_chunked_reference(&session_entry(&spec).unwrap().get_password().unwrap())
                .is_some()
        );
        assert_eq!(load_session(&spec).unwrap(), session);
        clear_session(&spec).unwrap();
        assert!(matches!(
            session_entry(&spec).unwrap().get_password(),
            Err(keyring::Error::NoEntry)
        ));
    }

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
