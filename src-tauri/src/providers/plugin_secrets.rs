use serde_json::Value;

use super::credential_store::{CredentialKey, CredentialStoreHandle};
use super::plugin::PluginRuntimeSpec;

const LEGACY_SESSION_FILE: &str = "session.dpapi";

pub fn load_session(spec: &PluginRuntimeSpec) -> Result<Value, String> {
    load_session_with_store(spec, &CredentialStoreHandle::default())
}

pub fn save_session(spec: &PluginRuntimeSpec, session: &Value) -> Result<(), String> {
    save_session_with_store(spec, session, &CredentialStoreHandle::default())
}

pub fn clear_session(spec: &PluginRuntimeSpec) -> Result<(), String> {
    clear_session_with_store(spec, &CredentialStoreHandle::default())?;
    // 旧 DPAPI 文件只在用户显式清除会话或卸载插件时随插件数据一起删除；启动时
    // 不再调用任何平台凭据 API，也不会尝试解密或导入它。
    let legacy = spec.data_dir.join(LEGACY_SESSION_FILE);
    if legacy.exists() {
        std::fs::remove_file(legacy).map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn load_session_with_store(
    spec: &PluginRuntimeSpec,
    store: &CredentialStoreHandle,
) -> Result<Value, String> {
    let key = CredentialKey::plugin_session(&spec.plugin_id)?;
    let Some(value) = store.get(&key)? else {
        return Ok(Value::Null);
    };
    serde_json::from_str(&value).map_err(|_| "插件登录会话数据损坏".to_string())
}

fn save_session_with_store(
    spec: &PluginRuntimeSpec,
    session: &Value,
    store: &CredentialStoreHandle,
) -> Result<(), String> {
    let key = CredentialKey::plugin_session(&spec.plugin_id)?;
    let value = serde_json::to_string(session).map_err(|error| error.to_string())?;
    store.write_verified(&key, &value)
}

fn clear_session_with_store(
    spec: &PluginRuntimeSpec,
    store: &CredentialStoreHandle,
) -> Result<(), String> {
    let key = CredentialKey::plugin_session(&spec.plugin_id)?;
    store.store().delete(&key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::credential_store::LocalCredentialStore;
    use std::sync::Arc;

    fn test_spec(root: &std::path::Path, plugin_id: &str) -> PluginRuntimeSpec {
        PluginRuntimeSpec {
            plugin_id: plugin_id.into(),
            source_namespace: plugin_id.into(),
            capabilities: vec![],
            secret_fields: vec![],
            credentials: None,
            root: std::path::PathBuf::new(),
            entrypoint: std::path::PathBuf::new(),
            permissions: Vec::new(),
            allowed_hosts: Vec::new(),
            browser_session: None,
            data_dir: root.join("plugin-data").join(plugin_id),
            trust: "trusted".into(),
        }
    }

    fn test_store(name: &str) -> (std::path::PathBuf, CredentialStoreHandle) {
        let root = std::env::temp_dir().join(format!(
            "say-it-plugin-session-{name}-{}",
            uuid::Uuid::new_v4()
        ));
        let store = LocalCredentialStore::from_directory(root.join("credentials"));
        (root, CredentialStoreHandle::from_store(Arc::new(store)))
    }

    #[test]
    fn long_session_round_trips_through_local_encrypted_vault() {
        let (root, store) = test_store("round-trip");
        let spec = test_spec(&root, "com.example.session");
        let session = serde_json::json!({
            "cookies": [{ "name": "session", "value": "x".repeat(6_000) }]
        });
        save_session_with_store(&spec, &session, &store).unwrap();
        assert_eq!(load_session_with_store(&spec, &store).unwrap(), session);
        clear_session_with_store(&spec, &store).unwrap();
        assert_eq!(load_session_with_store(&spec, &store).unwrap(), Value::Null);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn plugin_sessions_are_namespaced_and_clear_does_not_touch_provider_credentials() {
        let (root, store) = test_store("isolation");
        let first = test_spec(&root, "com.example.first");
        let second = test_spec(&root, "com.example.second");
        let provider_key = CredentialKey::plugin("com.example.first", "shared", "apiKey").unwrap();
        store.store().set(&provider_key, "retained").unwrap();
        save_session_with_store(&first, &serde_json::json!({"token":"first"}), &store).unwrap();
        save_session_with_store(&second, &serde_json::json!({"token":"second"}), &store).unwrap();

        clear_session_with_store(&first, &store).unwrap();

        assert_eq!(
            load_session_with_store(&first, &store).unwrap(),
            Value::Null
        );
        assert_eq!(
            load_session_with_store(&second, &store).unwrap(),
            serde_json::json!({"token":"second"})
        );
        assert_eq!(
            store.get(&provider_key).unwrap().as_deref(),
            Some("retained")
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn legacy_platform_file_is_never_read_during_load() {
        let (root, store) = test_store("legacy-not-read");
        let spec = test_spec(&root, "com.example.legacy");
        std::fs::create_dir_all(&spec.data_dir).unwrap();
        std::fs::write(
            spec.data_dir.join(LEGACY_SESSION_FILE),
            b"not-a-secret-store",
        )
        .unwrap();

        assert_eq!(load_session_with_store(&spec, &store).unwrap(), Value::Null);
        assert!(spec.data_dir.join(LEGACY_SESSION_FILE).exists());
        std::fs::remove_dir_all(root).unwrap();
    }
}
