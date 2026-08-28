use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use super::credential_vault::LocalCredentialVault;

static LOCAL_VAULT_DIRECTORY: OnceLock<PathBuf> = OnceLock::new();

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CredentialKey {
    owner: CredentialOwner,
    field: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum CredentialOwner {
    Provider {
        provider_id: String,
    },
    Plugin {
        plugin_id: String,
        provider_id: String,
    },
    PluginSession {
        plugin_id: String,
    },
}

impl CredentialKey {
    pub fn provider(provider_id: &str, field: &str) -> Result<Self, String> {
        validate_component("providerId", provider_id)?;
        validate_component("field", field)?;
        Ok(Self {
            owner: CredentialOwner::Provider {
                provider_id: provider_id.to_string(),
            },
            field: field.to_string(),
        })
    }

    pub fn plugin(plugin_id: &str, provider_id: &str, field: &str) -> Result<Self, String> {
        validate_component("pluginId", plugin_id)?;
        validate_component("providerId", provider_id)?;
        validate_component("field", field)?;
        Ok(Self {
            owner: CredentialOwner::Plugin {
                plugin_id: plugin_id.to_string(),
                provider_id: provider_id.to_string(),
            },
            field: field.to_string(),
        })
    }

    pub(crate) fn plugin_session(plugin_id: &str) -> Result<Self, String> {
        validate_component("pluginId", plugin_id)?;
        Ok(Self {
            owner: CredentialOwner::PluginSession {
                plugin_id: plugin_id.to_string(),
            },
            field: "browserSession".to_string(),
        })
    }

    fn account(&self) -> String {
        match &self.owner {
            CredentialOwner::Provider { provider_id } => {
                format!("credential:v1:provider:{provider_id}:{}", self.field)
            }
            CredentialOwner::Plugin {
                plugin_id,
                provider_id,
            } => format!(
                "credential:v1:plugin:{plugin_id}:{provider_id}:{}",
                self.field
            ),
            CredentialOwner::PluginSession { plugin_id } => {
                format!("credential:v1:plugin-session:{plugin_id}:{}", self.field)
            }
        }
    }

    fn unambiguous_legacy_account(&self) -> Option<String> {
        match &self.owner {
            CredentialOwner::Provider { provider_id } => {
                Some(format!("provider:{provider_id}:{}", self.field))
            }
            CredentialOwner::Plugin {
                plugin_id,
                provider_id,
            } if plugin_id != "session" => {
                Some(format!("plugin-{plugin_id}:{provider_id}:{}", self.field))
            }
            CredentialOwner::Plugin { .. } | CredentialOwner::PluginSession { .. } => None,
        }
    }
}

fn validate_component(label: &str, value: &str) -> Result<(), String> {
    if value.trim().is_empty()
        || value.len() > 160
        || value.chars().any(|ch| ch == ':' || ch.is_control())
    {
        return Err(format!("凭据 {label} 不合法"));
    }
    Ok(())
}

pub trait CredentialStore: Send + Sync {
    fn get(&self, key: &CredentialKey) -> Result<Option<String>, String>;
    fn set(&self, key: &CredentialKey, value: &str) -> Result<(), String>;
    fn delete(&self, key: &CredentialKey) -> Result<(), String>;

    fn write_verified(&self, key: &CredentialKey, value: &str) -> Result<(), String> {
        let previous = self.get(key)?;
        self.set(key, value)?;
        match self.get(key) {
            Ok(Some(actual)) if actual == value => Ok(()),
            verification => {
                let restore = match previous {
                    Some(previous) => self.set(key, &previous),
                    None => self.delete(key),
                };
                let reason = match verification {
                    Ok(_) => "本地加密凭据写入后校验不一致".to_string(),
                    Err(error) => format!("本地加密凭据写入后校验失败：{error}"),
                };
                restore.map_err(|error| format!("{reason}；恢复旧凭据失败：{error}"))?;
                Err(reason)
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct LocalCredentialStore {
    directory: Option<PathBuf>,
}

impl Default for LocalCredentialStore {
    fn default() -> Self {
        #[cfg(test)]
        {
            let directory = std::env::var_os("SAY_IT_LOCAL_VAULT_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    std::env::temp_dir().join(format!(
                        "say-it-test-vault-{}-{}",
                        std::process::id(),
                        uuid::Uuid::new_v4()
                    ))
                });
            return Self {
                directory: Some(directory),
            };
        }
        #[cfg(not(test))]
        Self { directory: None }
    }
}

impl LocalCredentialStore {
    #[cfg(test)]
    pub(crate) fn from_directory(directory: PathBuf) -> Self {
        Self {
            directory: Some(directory),
        }
    }

    fn vault(&self) -> Result<LocalCredentialVault, String> {
        let directory = self
            .directory
            .clone()
            .or_else(|| LOCAL_VAULT_DIRECTORY.get().cloned())
            .ok_or("本地加密凭据库尚未初始化")?;
        LocalCredentialVault::open(directory)
    }
}

pub(crate) fn configure_local_vault(directory: &Path) -> Result<(), String> {
    match LOCAL_VAULT_DIRECTORY.set(directory.to_path_buf()) {
        Ok(()) => {}
        Err(requested) if LOCAL_VAULT_DIRECTORY.get() == Some(&requested) => {}
        Err(_) => return Err("本地加密凭据库不能在运行期间切换目录".into()),
    }
    LocalCredentialVault::open(directory.to_path_buf()).map(|_| ())
}

impl CredentialStore for LocalCredentialStore {
    fn get(&self, key: &CredentialKey) -> Result<Option<String>, String> {
        let vault = self.vault()?;
        match vault.get(&key.account())? {
            Some(value) => Ok(Some(value)),
            None => match key.unambiguous_legacy_account() {
                Some(legacy) => vault.get(&legacy),
                None => Ok(None),
            },
        }
    }

    fn set(&self, key: &CredentialKey, value: &str) -> Result<(), String> {
        self.vault()?.set(&key.account(), value)
    }

    fn delete(&self, key: &CredentialKey) -> Result<(), String> {
        let vault = self.vault()?;
        vault.delete(&key.account())?;
        if let Some(legacy) = key.unambiguous_legacy_account() {
            vault.delete(&legacy)?;
        }
        Ok(())
    }

    fn write_verified(&self, key: &CredentialKey, value: &str) -> Result<(), String> {
        let vault = self.vault()?;
        vault.write_verified(&key.account(), value)
    }
}

#[derive(Clone)]
pub struct CredentialStoreHandle(Arc<dyn CredentialStore>);

impl Default for CredentialStoreHandle {
    fn default() -> Self {
        Self(Arc::new(LocalCredentialStore::default()))
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
        self.0.write_verified(key, value)
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
    fn structured_accounts_keep_provider_plugin_and_plugin_session_disjoint() {
        let provider = CredentialKey::provider("victim", "browserSession").unwrap();
        let plugin = CredentialKey::plugin("session", "victim", "browserSession").unwrap();
        let plugin_session = CredentialKey::plugin_session("victim").unwrap();

        assert_ne!(provider.account(), plugin.account());
        assert_ne!(provider.account(), plugin_session.account());
        assert_ne!(plugin.account(), plugin_session.account());
        assert!(plugin.unambiguous_legacy_account().is_none());
        assert!(plugin_session.unambiguous_legacy_account().is_none());
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

    #[test]
    fn production_credential_modules_have_no_system_store_or_prompt_path() {
        let manifest = include_str!("../../Cargo.toml");
        let sources = concat!(
            include_str!("credential_store.rs"),
            include_str!("credential_vault.rs"),
            include_str!("plugin_secrets.rs")
        );
        let forbidden = [
            ["key", "ring ="].concat(),
            ["key", "ring::"].concat(),
            ["get_", "password("].concat(),
            ["set_", "password("].concat(),
            ["Crypt", "UnprotectData"].concat(),
            ["Security", ".framework"].concat(),
        ];
        for token in forbidden {
            assert!(
                !manifest.contains(&token),
                "Cargo.toml 仍包含系统凭据依赖：{token}"
            );
            assert!(
                !sources.contains(&token),
                "凭据实现仍包含系统凭据调用：{token}"
            );
        }
    }
}
