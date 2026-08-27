use std::sync::Arc;

const CREDENTIAL_SERVICE: &str = "com.henjicc.sayit.credentials";

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CredentialKey {
    namespace: String,
    provider_id: String,
    field: String,
}

impl CredentialKey {
    pub fn provider(provider_id: &str, field: &str) -> Result<Self, String> {
        Self::new("provider", provider_id, field)
    }

    pub fn plugin(plugin_id: &str, provider_id: &str, field: &str) -> Result<Self, String> {
        Self::new(&format!("plugin-{plugin_id}"), provider_id, field)
    }

    fn new(namespace: &str, provider_id: &str, field: &str) -> Result<Self, String> {
        for (label, value) in [
            ("namespace", namespace),
            ("providerId", provider_id),
            ("field", field),
        ] {
            if value.trim().is_empty()
                || value.len() > 160
                || value.chars().any(|ch| ch == ':' || ch.is_control())
            {
                return Err(format!("凭据 {label} 不合法"));
            }
        }
        Ok(Self {
            namespace: namespace.to_string(),
            provider_id: provider_id.to_string(),
            field: field.to_string(),
        })
    }

    fn account(&self) -> String {
        format!("{}:{}:{}", self.namespace, self.provider_id, self.field)
    }
}

pub trait CredentialStore: Send + Sync {
    fn get(&self, key: &CredentialKey) -> Result<Option<String>, String>;
    fn set(&self, key: &CredentialKey, value: &str) -> Result<(), String>;
    fn delete(&self, key: &CredentialKey) -> Result<(), String>;
}

#[derive(Default)]
pub struct SystemCredentialStore;

impl CredentialStore for SystemCredentialStore {
    fn get(&self, key: &CredentialKey) -> Result<Option<String>, String> {
        #[cfg(any(target_os = "macos", windows))]
        {
            let entry = keyring::Entry::new(CREDENTIAL_SERVICE, &key.account())
                .map_err(|error| format!("打开系统凭据项失败：{error}"))?;
            return match entry.get_password() {
                Ok(value) => Ok(Some(value)),
                Err(keyring::Error::NoEntry) => Ok(None),
                Err(error) => Err(format!("读取系统凭据失败：{error}")),
            };
        }
        #[cfg(not(any(target_os = "macos", windows)))]
        {
            let _ = key;
            Err("当前 Linux 构建未配置系统凭据库，不能保存在线供应商密钥".into())
        }
    }

    fn set(&self, key: &CredentialKey, value: &str) -> Result<(), String> {
        #[cfg(any(target_os = "macos", windows))]
        {
            return keyring::Entry::new(CREDENTIAL_SERVICE, &key.account())
                .map_err(|error| format!("打开系统凭据项失败：{error}"))?
                .set_password(value)
                .map_err(|error| format!("写入系统凭据失败：{error}"));
        }
        #[cfg(not(any(target_os = "macos", windows)))]
        {
            let _ = (key, value);
            Err("当前 Linux 构建未配置系统凭据库，不能保存在线供应商密钥".into())
        }
    }

    fn delete(&self, key: &CredentialKey) -> Result<(), String> {
        #[cfg(any(target_os = "macos", windows))]
        {
            let entry = keyring::Entry::new(CREDENTIAL_SERVICE, &key.account())
                .map_err(|error| format!("打开系统凭据项失败：{error}"))?;
            return match entry.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                Err(error) => Err(format!("删除系统凭据失败：{error}")),
            };
        }
        #[cfg(not(any(target_os = "macos", windows)))]
        {
            let _ = key;
            Err("当前 Linux 构建未配置系统凭据库，不能删除在线供应商密钥".into())
        }
    }
}

#[derive(Clone)]
pub struct CredentialStoreHandle(Arc<dyn CredentialStore>);

impl Default for CredentialStoreHandle {
    fn default() -> Self {
        Self(Arc::new(SystemCredentialStore))
    }
}

impl CredentialStoreHandle {
    #[cfg(test)]
    pub fn from_store(store: Arc<dyn CredentialStore>) -> Self {
        Self(store)
    }

    pub fn get(&self, key: &CredentialKey) -> Result<Option<String>, String> {
        self.0.get(key)
    }

    pub fn write_verified(&self, key: &CredentialKey, value: &str) -> Result<(), String> {
        let previous = self.0.get(key)?;
        self.0.set(key, value)?;
        match self.0.get(key) {
            Ok(Some(actual)) if actual == value => Ok(()),
            verification => {
                let restore = match previous {
                    Some(previous) => self.0.set(key, &previous),
                    None => self.0.delete(key),
                };
                let reason = match verification {
                    Ok(_) => "系统凭据写入后校验不一致".to_string(),
                    Err(error) => format!("系统凭据写入后校验失败：{error}"),
                };
                restore.map_err(|error| format!("{reason}；恢复旧凭据失败：{error}"))?;
                Err(reason)
            }
        }
    }

    pub(crate) fn store(&self) -> &dyn CredentialStore {
        self.0.as_ref()
    }
}

pub fn key_for_profile(
    profile: &super::ProviderProfile,
    field: &str,
) -> Result<CredentialKey, String> {
    if let Some(plugin_id) = profile.kind.strip_prefix("plugin:") {
        CredentialKey::plugin(plugin_id, &profile.id, field)
    } else {
        CredentialKey::provider(&profile.id, field)
    }
}

pub fn redact_error(error: &str, secrets: &[String]) -> String {
    secrets
        .iter()
        .filter(|secret| !secret.is_empty())
        .fold(error.to_string(), |message, secret| {
            message.replace(secret, "[REDACTED]")
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    struct FakeStore {
        values: Mutex<HashMap<CredentialKey, String>>,
        fail_verification: Mutex<bool>,
        deletes: Mutex<usize>,
        writes: Mutex<usize>,
    }

    impl CredentialStore for FakeStore {
        fn get(&self, key: &CredentialKey) -> Result<Option<String>, String> {
            if *self.fail_verification.lock().unwrap() && *self.writes.lock().unwrap() > 0 {
                return Err("fake read failure".into());
            }
            Ok(self.values.lock().unwrap().get(key).cloned())
        }

        fn set(&self, key: &CredentialKey, value: &str) -> Result<(), String> {
            *self.writes.lock().unwrap() += 1;
            self.values
                .lock()
                .unwrap()
                .insert(key.clone(), value.to_string());
            Ok(())
        }

        fn delete(&self, key: &CredentialKey) -> Result<(), String> {
            *self.deletes.lock().unwrap() += 1;
            self.values.lock().unwrap().remove(key);
            Ok(())
        }
    }

    #[test]
    fn verified_write_restores_previous_value_after_verification_failure() {
        let fake = Arc::new(FakeStore::default());
        let key = CredentialKey::provider("bailian", "apiKey").unwrap();
        fake.set(&key, "old").unwrap();
        *fake.writes.lock().unwrap() = 0;
        let handle = CredentialStoreHandle::from_store(fake.clone());
        *fake.fail_verification.lock().unwrap() = true;
        let error = handle.write_verified(&key, "new").unwrap_err();
        assert!(error.contains("校验失败"));
        *fake.fail_verification.lock().unwrap() = false;
        assert_eq!(fake.get(&key).unwrap().as_deref(), Some("old"));
    }

    #[test]
    fn plugin_credentials_use_isolated_namespace() {
        let provider = super::super::ProviderProfile {
            id: "shared".into(),
            kind: "plugin:demo".into(),
            display_name: "Demo".into(),
            auth_kind: "api-key".into(),
            capabilities: vec![],
            enabled: true,
            config: serde_json::json!({}),
            config_fields: vec![],
            actions: vec![],
        };
        assert_ne!(
            key_for_profile(&provider, "apiKey").unwrap(),
            CredentialKey::provider("shared", "apiKey").unwrap()
        );
    }

    #[test]
    fn errors_are_redacted_without_exposing_secret() {
        let secret = "sk-sensitive-value".to_string();
        let redacted = redact_error(&format!("write failed for {secret}"), &[secret.clone()]);
        assert!(!redacted.contains(&secret));
        assert!(redacted.contains("[REDACTED]"));
    }

    #[test]
    fn removing_provider_profile_does_not_delete_credential() {
        let fake = Arc::new(FakeStore::default());
        let key = CredentialKey::provider("temporary", "apiKey").unwrap();
        fake.set(&key, "retained").unwrap();
        let mut settings = super::super::ProviderSettings::default();
        settings.profiles.push(super::super::ProviderProfile {
            id: "temporary".into(),
            kind: "llm:custom".into(),
            display_name: "Temporary".into(),
            auth_kind: "api-key".into(),
            capabilities: vec!["llm".into()],
            enabled: true,
            config: serde_json::json!({}),
            config_fields: vec![],
            actions: vec![],
        });

        super::super::remove_profile_preserving_credentials(&mut settings, "temporary");

        assert!(settings
            .profiles
            .iter()
            .all(|profile| profile.id != "temporary"));
        assert_eq!(fake.get(&key).unwrap().as_deref(), Some("retained"));
        assert_eq!(*fake.deletes.lock().unwrap(), 0);
    }
}
