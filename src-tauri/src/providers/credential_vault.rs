use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use aes_gcm::aead::{Aead, Payload};
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use fs2::FileExt;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

const VAULT_VERSION: u32 = 1;
const VAULT_ALGORITHM: &str = "AES-256-GCM";
const MASTER_KEY_BYTES: usize = 32;
const NONCE_BYTES: usize = 12;
const MASTER_KEY_FILE: &str = "master.key";
const VAULT_FILE: &str = "vault.json";
const VAULT_LOCK_FILE: &str = ".vault.lock";
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

struct VaultLock {
    file: File,
    _process_guard: MutexGuard<'static, ()>,
}

impl Drop for VaultLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

#[derive(Debug)]
enum VaultReadError {
    Io(String),
    Corrupt(String),
    Unsupported(String),
}

impl VaultReadError {
    fn message(self) -> String {
        match self {
            Self::Io(message) | Self::Corrupt(message) | Self::Unsupported(message) => message,
        }
    }
}

impl LocalCredentialVault {
    pub(super) fn open(directory: PathBuf) -> Result<Self, String> {
        ensure_private_directory(&directory)?;
        let vault = Self { directory };
        let _guard = vault.lock()?;
        vault.load_or_create_master_key_locked()?;
        Ok(vault)
    }

    pub(super) fn get(&self, account: &str) -> Result<Option<String>, String> {
        let _guard = self.lock()?;
        Ok(self.load_contents_locked()?.entries.get(account).cloned())
    }

    pub(super) fn set(&self, account: &str, value: &str) -> Result<(), String> {
        let _guard = self.lock()?;
        let mut contents = self.load_contents_locked()?;
        contents
            .entries
            .insert(account.to_string(), value.to_string());
        self.write_contents_locked(&contents)
    }

    pub(super) fn delete(&self, account: &str) -> Result<(), String> {
        let _guard = self.lock()?;
        let mut contents = self.load_contents_locked()?;
        if contents.entries.remove(account).is_some() {
            self.write_contents_locked(&contents)?;
        }
        Ok(())
    }

    pub(super) fn write_verified(&self, account: &str, value: &str) -> Result<(), String> {
        let _guard = self.lock()?;
        let mut contents = self.load_contents_locked()?;
        let original = contents.entries.clone();
        contents
            .entries
            .insert(account.to_string(), value.to_string());
        self.write_contents_locked(&contents)?;
        let verification = self.load_contents_locked();
        if matches!(verification, Ok(ref current) if current.entries.get(account).map(String::as_str) == Some(value))
        {
            return Ok(());
        }
        self.write_contents_locked(&VaultContents { entries: original })?;
        let reason = match verification {
            Ok(_) => "本地加密凭据写入后校验不一致".to_string(),
            Err(error) => format!("本地加密凭据写入后校验失败：{error}"),
        };
        Err(reason)
    }

    fn lock(&self) -> Result<VaultLock, String> {
        let process_guard = VAULT_IO_LOCK
            .lock()
            .map_err(|_| "本地加密凭据库进程内锁已损坏".to_string())?;
        let path = self.directory.join(VAULT_LOCK_FILE);
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options
            .open(&path)
            .map_err(|error| format!("打开本地凭据库跨进程锁失败：{error}"))?;
        set_private_file_permissions(&path)?;
        file.lock_exclusive()
            .map_err(|error| format!("获取本地凭据库跨进程锁失败：{error}"))?;
        Ok(VaultLock {
            file,
            _process_guard: process_guard,
        })
    }

    fn key_path(&self) -> PathBuf {
        self.directory.join(MASTER_KEY_FILE)
    }

    fn vault_path(&self) -> PathBuf {
        self.directory.join(VAULT_FILE)
    }

    fn load_or_create_master_key_locked(&self) -> Result<Zeroizing<Vec<u8>>, String> {
        let path = self.key_path();
        if path.exists() {
            if fs::metadata(&path)
                .map(|metadata| metadata.len())
                .unwrap_or(0)
                == MASTER_KEY_BYTES as u64
            {
                return read_master_key(&path);
            }
            if self.vault_path().exists() || backup_path(&self.vault_path()).exists() {
                return Err("本地凭据主密钥长度异常，且已有 vault，拒绝重新生成".into());
            }
            remove_file_durably(&path)
                .map_err(|error| format!("清理首次创建中断的主密钥失败：{error}"))?;
        }

        if self.vault_path().exists() || backup_path(&self.vault_path()).exists() {
            return Err("本地凭据主密钥缺失，拒绝生成新密钥覆盖现有 vault".into());
        }

        let mut key = Zeroizing::new(vec![0_u8; MASTER_KEY_BYTES]);
        rand::rng().fill_bytes(&mut key);
        write_private_file(&path, &key, true)?;
        sync_directory(&self.directory)?;
        Ok(key)
    }

    fn load_contents_locked(&self) -> Result<VaultContents, String> {
        let key = self.load_or_create_master_key_locked()?;
        let path = self.vault_path();
        if !path.exists() {
            let backup = backup_path(&path);
            if backup.exists() {
                let contents = decrypt_file(&backup, &key).map_err(VaultReadError::message)?;
                restore_primary_from_backup(&path, &backup)?;
                return Ok(contents);
            }
            return Ok(VaultContents::default());
        }

        match decrypt_file(&path, &key) {
            Ok(contents) => Ok(contents),
            Err(VaultReadError::Unsupported(message)) => Err(message),
            Err(VaultReadError::Io(message)) => Err(message),
            Err(VaultReadError::Corrupt(primary_error)) => {
                let backup = backup_path(&path);
                if !backup.exists() {
                    return Err(primary_error);
                }
                let contents =
                    decrypt_file(&backup, &key).map_err(|backup_error| match backup_error {
                        VaultReadError::Unsupported(message) => message,
                        error => format!(
                            "本地加密凭据库主文件与备份均不可用：{primary_error}；{}",
                            error.message()
                        ),
                    })?;
                restore_primary_from_backup(&path, &backup)?;
                Ok(contents)
            }
        }
    }

    fn write_contents_locked(&self, contents: &VaultContents) -> Result<(), String> {
        let key = self.load_or_create_master_key_locked()?;
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

fn decrypt_file(path: &Path, key: &[u8]) -> Result<VaultContents, VaultReadError> {
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|mut file| file.read_to_end(&mut bytes))
        .map_err(|error| VaultReadError::Io(format!("读取本地加密凭据库失败：{error}")))?;
    let envelope: VaultEnvelope = serde_json::from_slice(&bytes)
        .map_err(|_| VaultReadError::Corrupt("本地加密凭据库格式损坏".to_string()))?;
    if envelope.version != VAULT_VERSION || envelope.algorithm != VAULT_ALGORITHM {
        return Err(VaultReadError::Unsupported(
            "本地加密凭据库版本或算法不受支持，已保留原文件".into(),
        ));
    }
    let nonce = BASE64
        .decode(envelope.nonce)
        .map_err(|_| VaultReadError::Corrupt("本地加密凭据库 nonce 损坏".to_string()))?;
    if nonce.len() != NONCE_BYTES {
        return Err(VaultReadError::Corrupt(
            "本地加密凭据库 nonce 长度异常".into(),
        ));
    }
    let ciphertext = BASE64
        .decode(envelope.ciphertext)
        .map_err(|_| VaultReadError::Corrupt("本地加密凭据库密文损坏".to_string()))?;
    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|_| VaultReadError::Corrupt("本地凭据主密钥长度异常".to_string()))?;
    let plaintext = Zeroizing::new(
        cipher
            .decrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: &ciphertext,
                    aad: VAULT_AAD,
                },
            )
            .map_err(|_| {
                VaultReadError::Corrupt("本地加密凭据库认证失败，密钥错误或文件被篡改".to_string())
            })?,
    );
    serde_json::from_slice(&plaintext)
        .map_err(|_| VaultReadError::Corrupt("本地加密凭据库明文格式损坏".to_string()))
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
        let backup_temporary = backup.with_extension(format!("bak.{}.tmp", uuid::Uuid::new_v4()));
        let previous =
            fs::read(path).map_err(|error| format!("读取本地凭据库备份源失败：{error}"))?;
        write_private_file(&backup_temporary, &previous, true)?;
        if let Err(error) = replace_file(&backup_temporary, &backup) {
            let _ = fs::remove_file(&temporary);
            let _ = fs::remove_file(&backup_temporary);
            return Err(format!("提交本地凭据库备份失败：{error}"));
        }
    }
    if let Err(error) = replace_file(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(format!("提交本地凭据库失败，原主文件保持不变：{error}"));
    }
    set_private_file_permissions(path)?;
    Ok(())
}

fn restore_primary_from_backup(path: &Path, backup: &Path) -> Result<(), String> {
    let bytes = fs::read(backup).map_err(|error| format!("读取本地凭据库备份失败：{error}"))?;
    let temporary = path.with_extension(format!("json.recovery.{}.tmp", uuid::Uuid::new_v4()));
    write_private_file(&temporary, &bytes, true)?;
    if let Err(error) = replace_file(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(format!("恢复本地凭据库失败，备份保持不变：{error}"));
    }
    set_private_file_permissions(path)?;
    Ok(())
}

pub(crate) fn write_private_file_atomically(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("file");
    let temporary = path.with_extension(format!("{extension}.{}.tmp", uuid::Uuid::new_v4()));
    write_private_file(&temporary, bytes, true)?;
    if let Err(error) = replace_file(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    set_private_file_permissions(path)
}

pub(crate) fn remove_file_durably(path: &Path) -> Result<(), String> {
    match fs::remove_file(path) {
        Ok(()) => sync_directory(path.parent().ok_or("删除文件的父目录无效")?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("删除文件失败：{error}")),
    }
}

fn replace_file(source: &Path, destination: &Path) -> Result<(), String> {
    #[cfg(test)]
    if should_fail_replace(destination) {
        return Err("测试注入的原子替换失败".into());
    }

    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows::core::PCWSTR;
        use windows::Win32::Storage::FileSystem::{
            MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
        };
        let source = source
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let destination = destination
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        unsafe {
            MoveFileExW(
                PCWSTR(source.as_ptr()),
                PCWSTR(destination.as_ptr()),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        }
        .map_err(|error| format!("原子替换文件失败：{error}"))?;
    }
    #[cfg(not(windows))]
    fs::rename(source, destination).map_err(|error| format!("原子替换文件失败：{error}"))?;

    sync_directory(destination.parent().ok_or("原子替换目标目录无效")?)
}

#[cfg(test)]
static REPLACE_FAILURES: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

#[cfg(test)]
pub(crate) fn fail_next_replace(path: &Path) {
    REPLACE_FAILURES.lock().unwrap().push(path.to_path_buf());
}

#[cfg(test)]
fn should_fail_replace(path: &Path) -> bool {
    let mut failures = REPLACE_FAILURES.lock().unwrap();
    let Some(index) = failures.iter().position(|candidate| candidate == path) else {
        return false;
    };
    failures.remove(index);
    true
}

pub(crate) fn sync_directory(path: &Path) -> Result<(), String> {
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
    fn missing_primary_recovers_authenticated_backup_instead_of_opening_empty() {
        let (root, vault) = test_vault("missing-primary");
        vault.set("provider:test:key", "old").unwrap();
        vault.set("provider:test:key", "new").unwrap();
        fs::remove_file(root.join(VAULT_FILE)).unwrap();

        assert_eq!(
            vault.get("provider:test:key").unwrap().as_deref(),
            Some("old")
        );
        assert!(root.join(VAULT_FILE).is_file());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_backup_restore_is_explicit_and_remains_retryable() {
        let (root, vault) = test_vault("restore-failure");
        vault.set("provider:test:key", "old").unwrap();
        vault.set("provider:test:key", "new").unwrap();
        let primary = root.join(VAULT_FILE);
        let backup = backup_path(&primary);
        fs::remove_file(&primary).unwrap();
        fail_next_replace(&primary);

        let error = vault.get("provider:test:key").unwrap_err();

        assert!(error.contains("恢复"));
        assert!(!primary.exists());
        assert!(backup.exists());
        assert_eq!(
            vault.get("provider:test:key").unwrap().as_deref(),
            Some("old")
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unsupported_primary_version_is_preserved_and_never_downgraded_from_backup() {
        let (root, vault) = test_vault("unsupported-version");
        vault.set("provider:test:key", "old").unwrap();
        vault.set("provider:test:key", "new").unwrap();
        let path = root.join(VAULT_FILE);
        let mut envelope: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        envelope["version"] = serde_json::json!(VAULT_VERSION + 1);
        let unsupported = serde_json::to_vec_pretty(&envelope).unwrap();
        fs::write(&path, &unsupported).unwrap();

        let error = vault.get("provider:test:key").unwrap_err();

        assert!(error.contains("不受支持"));
        assert_eq!(fs::read(&path).unwrap(), unsupported);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_primary_replace_keeps_last_complete_vault() {
        let (root, vault) = test_vault("replace-failure");
        vault.set("provider:test:first", "one").unwrap();
        vault.set("provider:test:second", "two").unwrap();
        fail_next_replace(&root.join(VAULT_FILE));

        assert!(vault.set("provider:test:third", "three").is_err());
        assert_eq!(
            vault.get("provider:test:first").unwrap().as_deref(),
            Some("one")
        );
        assert_eq!(
            vault.get("provider:test:second").unwrap().as_deref(),
            Some("two")
        );
        assert_eq!(vault.get("provider:test:third").unwrap(), None);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn missing_master_key_never_regenerates_over_existing_vault() {
        let (root, vault) = test_vault("missing-key");
        vault.set("provider:test:key", "value").unwrap();
        fs::remove_file(root.join(MASTER_KEY_FILE)).unwrap();

        let error = LocalCredentialVault::open(root.clone()).unwrap_err();

        assert!(error.contains("拒绝生成新密钥"));
        assert!(!root.join(MASTER_KEY_FILE).exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn interrupted_first_master_key_creation_is_retried_only_without_vault() {
        let root = std::env::temp_dir().join(format!(
            "say-it-vault-interrupted-key-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join(MASTER_KEY_FILE), [7_u8; 11]).unwrap();

        let vault = LocalCredentialVault::open(root.clone()).unwrap();

        assert_eq!(
            fs::read(root.join(MASTER_KEY_FILE)).unwrap().len(),
            MASTER_KEY_BYTES
        );
        vault.set("provider:test:key", "value").unwrap();
        assert_eq!(
            vault.get("provider:test:key").unwrap().as_deref(),
            Some("value")
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cross_process_worker() {
        let Some(root) = std::env::var_os("SAY_IT_VAULT_PROCESS_TEST_DIR") else {
            return;
        };
        let account = std::env::var("SAY_IT_VAULT_PROCESS_TEST_ACCOUNT").unwrap();
        let value = std::env::var("SAY_IT_VAULT_PROCESS_TEST_VALUE").unwrap();
        let vault = LocalCredentialVault::open(PathBuf::from(root)).unwrap();
        vault.set(&account, &value).unwrap();
    }

    #[test]
    fn separate_processes_share_one_key_and_do_not_lose_updates() {
        let root = std::env::temp_dir().join(format!(
            "say-it-vault-processes-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let executable = std::env::current_exe().unwrap();
        let mut children = (0..8)
            .map(|index| {
                std::process::Command::new(&executable)
                    .args([
                        "--exact",
                        "providers::credential_vault::tests::cross_process_worker",
                        "--nocapture",
                    ])
                    .env("SAY_IT_VAULT_PROCESS_TEST_DIR", &root)
                    .env(
                        "SAY_IT_VAULT_PROCESS_TEST_ACCOUNT",
                        format!("provider:test:key-{index}"),
                    )
                    .env("SAY_IT_VAULT_PROCESS_TEST_VALUE", format!("value-{index}"))
                    .spawn()
                    .unwrap()
            })
            .collect::<Vec<_>>();
        for child in &mut children {
            assert!(child.wait().unwrap().success());
        }
        let vault = LocalCredentialVault::open(root.clone()).unwrap();
        for index in 0..8 {
            assert_eq!(
                vault
                    .get(&format!("provider:test:key-{index}"))
                    .unwrap()
                    .as_deref(),
                Some(format!("value-{index}").as_str())
            );
        }
        assert_eq!(
            fs::read(root.join(MASTER_KEY_FILE)).unwrap().len(),
            MASTER_KEY_BYTES
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
        assert_eq!(
            fs::metadata(root.join(VAULT_LOCK_FILE))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        fs::remove_dir_all(root).unwrap();
    }
}
