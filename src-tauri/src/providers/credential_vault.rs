use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use aes_gcm::aead::{Aead, Payload};
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

const VAULT_VERSION: u32 = 1;
const VAULT_ALGORITHM: &str = "AES-256-GCM";
const MASTER_KEY_BYTES: usize = 32;
const NONCE_BYTES: usize = 12;
const MASTER_KEY_FILE: &str = "master.key";
const VAULT_FILE: &str = "vault.json";
const VAULT_AAD: &[u8] = b"say-it-local-credential-vault:v1";
static VAULT_IO_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct VaultEnvelope {
    version: u32,
    algorithm: String,
    nonce: String,
    ciphertext: String,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct VaultContents {
    entries: BTreeMap<String, String>,
}

#[derive(Clone, Debug)]
pub(super) struct LocalCredentialVault {
    directory: PathBuf,
}

impl LocalCredentialVault {
    pub(super) fn open(directory: PathBuf) -> Result<Self, String> {
        ensure_private_directory(&directory)?;
        let vault = Self { directory };
        let _guard = VAULT_IO_LOCK
            .lock()
            .map_err(|_| "本地加密凭据库锁已损坏".to_string())?;
        vault.load_or_create_master_key()?;
        Ok(vault)
    }

    pub(super) fn get(&self, account: &str) -> Result<Option<String>, String> {
        let _guard = VAULT_IO_LOCK
            .lock()
            .map_err(|_| "本地加密凭据库锁已损坏".to_string())?;
        Ok(self.load_contents()?.entries.get(account).cloned())
    }

    pub(super) fn set(&self, account: &str, value: &str) -> Result<(), String> {
        let _guard = VAULT_IO_LOCK
            .lock()
            .map_err(|_| "本地加密凭据库锁已损坏".to_string())?;
        let mut contents = self.load_contents()?;
        contents
            .entries
            .insert(account.to_string(), value.to_string());
        self.write_contents(&contents)
    }

    pub(super) fn delete(&self, account: &str) -> Result<(), String> {
        let _guard = VAULT_IO_LOCK
            .lock()
            .map_err(|_| "本地加密凭据库锁已损坏".to_string())?;
        let mut contents = self.load_contents()?;
        if contents.entries.remove(account).is_some() {
            self.write_contents(&contents)?;
        }
        Ok(())
    }

    fn key_path(&self) -> PathBuf {
        self.directory.join(MASTER_KEY_FILE)
    }

    fn vault_path(&self) -> PathBuf {
        self.directory.join(VAULT_FILE)
    }

    fn load_or_create_master_key(&self) -> Result<Zeroizing<Vec<u8>>, String> {
        let path = self.key_path();
        if path.exists() {
            return read_master_key(&path);
        }

        let mut key = Zeroizing::new(vec![0_u8; MASTER_KEY_BYTES]);
        rand::rng().fill_bytes(&mut key);
        let temporary = self
            .directory
            .join(format!(".{MASTER_KEY_FILE}.{}.tmp", uuid::Uuid::new_v4()));
        write_private_file(&temporary, &key, true)?;
        match fs::rename(&temporary, &path) {
            Ok(()) => {
                set_private_file_permissions(&path)?;
                sync_directory(&self.directory)?;
                Ok(key)
            }
            Err(_) if path.exists() => {
                let _ = fs::remove_file(&temporary);
                read_master_key(&path)
            }
            Err(error) => {
                let _ = fs::remove_file(&temporary);
                Err(format!("提交本地凭据主密钥失败：{error}"))
            }
        }
    }

    fn load_contents(&self) -> Result<VaultContents, String> {
        let key = self.load_or_create_master_key()?;
        let path = self.vault_path();
        if !path.exists() {
            return Ok(VaultContents::default());
        }

        match decrypt_file(&path, &key) {
            Ok(contents) => Ok(contents),
            Err(primary_error) => {
                let backup = backup_path(&path);
                if !backup.exists() {
                    return Err(primary_error);
                }
                let contents = decrypt_file(&backup, &key).map_err(|backup_error| {
                    format!("本地加密凭据库主文件与备份均不可用：{primary_error}；{backup_error}")
                })?;
                restore_primary_from_backup(&path, &backup)?;
                Ok(contents)
            }
        }
    }

    fn write_contents(&self, contents: &VaultContents) -> Result<(), String> {
        let key = self.load_or_create_master_key()?;
        let plaintext = Zeroizing::new(
            serde_json::to_vec(contents).map_err(|error| format!("序列化凭据失败：{error}"))?,
        );
        let mut nonce = [0_u8; NONCE_BYTES];
        rand::rng().fill_bytes(&mut nonce);
        let cipher =
            Aes256Gcm::new_from_slice(&key).map_err(|_| "初始化本地凭据加密器失败".to_string())?;
        let ciphertext = cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: &plaintext,
                    aad: VAULT_AAD,
                },
            )
            .map_err(|_| "加密本地凭据失败".to_string())?;
        let envelope = VaultEnvelope {
            version: VAULT_VERSION,
            algorithm: VAULT_ALGORITHM.into(),
            nonce: BASE64.encode(nonce),
            ciphertext: BASE64.encode(ciphertext),
        };
        let bytes = serde_json::to_vec_pretty(&envelope)
            .map_err(|error| format!("序列化加密凭据文件失败：{error}"))?;
        atomic_write_with_backup(&self.vault_path(), &bytes)
    }
}

fn decrypt_file(path: &Path, key: &[u8]) -> Result<VaultContents, String> {
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|mut file| file.read_to_end(&mut bytes))
        .map_err(|error| format!("读取本地加密凭据库失败：{error}"))?;
    let envelope: VaultEnvelope =
        serde_json::from_slice(&bytes).map_err(|_| "本地加密凭据库格式损坏".to_string())?;
    if envelope.version != VAULT_VERSION || envelope.algorithm != VAULT_ALGORITHM {
        return Err("本地加密凭据库版本或算法不受支持".into());
    }
    let nonce = BASE64
        .decode(envelope.nonce)
        .map_err(|_| "本地加密凭据库 nonce 损坏".to_string())?;
    if nonce.len() != NONCE_BYTES {
        return Err("本地加密凭据库 nonce 长度异常".into());
    }
    let ciphertext = BASE64
        .decode(envelope.ciphertext)
        .map_err(|_| "本地加密凭据库密文损坏".to_string())?;
    let cipher =
        Aes256Gcm::new_from_slice(key).map_err(|_| "本地凭据主密钥长度异常".to_string())?;
    let plaintext = Zeroizing::new(
        cipher
            .decrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: &ciphertext,
                    aad: VAULT_AAD,
                },
            )
            .map_err(|_| "本地加密凭据库认证失败，密钥错误或文件被篡改".to_string())?,
    );
    serde_json::from_slice(&plaintext).map_err(|_| "本地加密凭据库明文格式损坏".to_string())
}

fn read_master_key(path: &Path) -> Result<Zeroizing<Vec<u8>>, String> {
    set_private_file_permissions(path)?;
    let key =
        Zeroizing::new(fs::read(path).map_err(|error| format!("读取本地凭据主密钥失败：{error}"))?);
    if key.len() != MASTER_KEY_BYTES {
        return Err("本地凭据主密钥长度异常".into());
    }
    Ok(key)
}

fn ensure_private_directory(path: &Path) -> Result<(), String> {
    fs::create_dir_all(path).map_err(|error| format!("创建本地凭据目录失败：{error}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .map_err(|error| format!("收紧本地凭据目录权限失败：{error}"))?;
    }
    Ok(())
}

fn write_private_file(path: &Path, bytes: &[u8], create_new: bool) -> Result<(), String> {
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(!create_new);
    if create_new {
        options.create_new(true);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|error| format!("创建本地凭据文件失败：{error}"))?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|error| format!("写入本地凭据文件失败：{error}"))?;
    set_private_file_permissions(path)
}

fn set_private_file_permissions(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .map_err(|error| format!("收紧本地凭据文件权限失败：{error}"))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn backup_path(path: &Path) -> PathBuf {
    path.with_extension("json.bak")
}

fn atomic_write_with_backup(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let temporary = path.with_extension(format!("json.{}.tmp", uuid::Uuid::new_v4()));
    write_private_file(&temporary, bytes, true)?;
    let backup = backup_path(path);
    if path.exists() {
        fs::copy(path, &backup).map_err(|error| format!("备份本地凭据库失败：{error}"))?;
        set_private_file_permissions(&backup)?;
        File::open(&backup)
            .and_then(|file| file.sync_all())
            .map_err(|error| format!("刷新本地凭据库备份失败：{error}"))?;
        #[cfg(windows)]
        fs::remove_file(path).map_err(|error| format!("替换本地凭据库失败：{error}"))?;
    }
    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        if !path.exists() && backup.exists() {
            let _ = fs::copy(&backup, path);
        }
        return Err(format!("提交本地凭据库失败，已尝试恢复备份：{error}"));
    }
    set_private_file_permissions(path)?;
    sync_directory(path.parent().ok_or("本地凭据库目录无效")?)
}

fn restore_primary_from_backup(path: &Path, backup: &Path) -> Result<(), String> {
    let bytes = fs::read(backup).map_err(|error| format!("读取本地凭据库备份失败：{error}"))?;
    let temporary = path.with_extension(format!("json.recovery.{}.tmp", uuid::Uuid::new_v4()));
    write_private_file(&temporary, &bytes, true)?;
    #[cfg(windows)]
    if path.exists() {
        fs::remove_file(path).map_err(|error| format!("恢复本地凭据库失败：{error}"))?;
    }
    fs::rename(&temporary, path).map_err(|error| format!("恢复本地凭据库失败：{error}"))?;
    set_private_file_permissions(path)?;
    sync_directory(path.parent().ok_or("本地凭据库目录无效")?)
}

fn sync_directory(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| format!("刷新本地凭据目录失败：{error}"))?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn test_vault(name: &str) -> (PathBuf, LocalCredentialVault) {
        let root =
            std::env::temp_dir().join(format!("say-it-vault-{name}-{}", uuid::Uuid::new_v4()));
        let vault = LocalCredentialVault::open(root.clone()).unwrap();
        (root, vault)
    }

    #[test]
    fn encrypted_round_trip_contains_no_plaintext_secret() {
        let (root, vault) = test_vault("round-trip");
        vault
            .set("provider:bailian:apiKey", "secret-fixture-value")
            .unwrap();
        assert_eq!(
            vault.get("provider:bailian:apiKey").unwrap().as_deref(),
            Some("secret-fixture-value")
        );
        assert!(
            !String::from_utf8_lossy(&fs::read(root.join(VAULT_FILE)).unwrap())
                .contains("secret-fixture-value")
        );
        assert!(
            !String::from_utf8_lossy(&fs::read(root.join(MASTER_KEY_FILE)).unwrap())
                .contains("secret-fixture-value")
        );
        let envelope =
            serde_json::from_slice::<serde_json::Value>(&fs::read(root.join(VAULT_FILE)).unwrap())
                .unwrap();
        let keys = envelope
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(keys.len(), 4);
        for key in ["version", "algorithm", "nonce", "ciphertext"] {
            assert!(envelope.get(key).is_some());
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn wrong_key_and_tampered_ciphertext_are_rejected() {
        let (root, vault) = test_vault("tamper");
        vault.set("provider:bailian:apiKey", "secret").unwrap();
        fs::write(root.join(MASTER_KEY_FILE), vec![7_u8; MASTER_KEY_BYTES]).unwrap();
        assert!(vault
            .get("provider:bailian:apiKey")
            .unwrap_err()
            .contains("认证失败"));

        fs::remove_file(root.join(VAULT_FILE)).unwrap();
        let reset = LocalCredentialVault::open(root.clone()).unwrap();
        reset.set("provider:bailian:apiKey", "secret").unwrap();
        fs::write(root.join(VAULT_FILE), b"{\"version\":1}").unwrap();
        assert!(reset.get("provider:bailian:apiKey").is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn valid_backup_recovers_corrupt_primary() {
        let (root, vault) = test_vault("backup");
        vault.set("provider:test:key", "old").unwrap();
        vault.set("provider:test:key", "new").unwrap();
        fs::write(root.join(VAULT_FILE), b"corrupt").unwrap();
        assert_eq!(
            vault.get("provider:test:key").unwrap().as_deref(),
            Some("old")
        );
        assert_eq!(
            vault.get("provider:test:key").unwrap().as_deref(),
            Some("old")
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn concurrent_updates_do_not_lose_entries() {
        let (root, vault) = test_vault("concurrent");
        let vault = Arc::new(vault);
        let threads = (0..12)
            .map(|index| {
                let vault = Arc::clone(&vault);
                std::thread::spawn(move || {
                    vault
                        .set(
                            &format!("provider:test:key-{index}"),
                            &format!("value-{index}"),
                        )
                        .unwrap();
                })
            })
            .collect::<Vec<_>>();
        for thread in threads {
            thread.join().unwrap();
        }
        for index in 0..12 {
            assert_eq!(
                vault
                    .get(&format!("provider:test:key-{index}"))
                    .unwrap()
                    .as_deref(),
                Some(format!("value-{index}").as_str())
            );
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[cfg(unix)]
    fn unix_directory_and_files_use_private_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let (root, vault) = test_vault("permissions");
        vault.set("provider:test:key", "value").unwrap();
        assert_eq!(
            fs::metadata(&root).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(root.join(MASTER_KEY_FILE))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(root.join(VAULT_FILE))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        fs::remove_dir_all(root).unwrap();
    }
}
