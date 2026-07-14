use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tauri::Manager;
use uuid::Uuid;
use zip::ZipArchive;

use super::plugin::{plugins_dir, validate_plugin_dir, PluginManifest, PluginSignatureManifest};

const TRUST_FILE: &str = "trusted-plugin-keys.json";
const BACKUPS_DIR: &str = "plugin-backups";
pub const SAYIT_PACKAGE_EXTENSION: &str = "sayit";
const PACKAGE_DECLARATION_FILE: &str = "sayit-package.json";
const MAX_ARCHIVE_ENTRIES: usize = 256;
const MAX_ARCHIVE_UNCOMPRESSED_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SayItPackageDeclaration {
    format_version: u32,
    kind: String,
    entry: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallResult {
    pub plugin_id: String,
    pub version: String,
    pub trust: String,
    pub replaced_version: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginBackup {
    pub plugin_id: String,
    pub version: String,
    pub directory: String,
    pub created_at_ms: u64,
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct TrustedKeyFile {
    #[serde(default)]
    keys: HashMap<String, String>,
}

pub fn load_trusted_keys(app: &tauri::AppHandle) -> Result<HashMap<String, String>, String> {
    let path = app
        .path()
        .app_local_data_dir()
        .map_err(|error| error.to_string())?
        .join(TRUST_FILE);
    if !path.exists() {
        return Ok(HashMap::new());
    }
    let text = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    serde_json::from_str::<TrustedKeyFile>(&text)
        .map(|file| file.keys)
        .map_err(|error| format!("插件信任库格式错误：{error}"))
}

fn save_trusted_keys(app: &tauri::AppHandle, keys: &HashMap<String, String>) -> Result<(), String> {
    let path = app
        .path()
        .app_local_data_dir()
        .map_err(|error| error.to_string())?
        .join(TRUST_FILE);
    let temp = path.with_extension(format!("tmp-{}", Uuid::new_v4()));
    let bytes = serde_json::to_vec_pretty(&TrustedKeyFile { keys: keys.clone() })
        .map_err(|error| error.to_string())?;
    std::fs::write(&temp, bytes).map_err(|error| error.to_string())?;
    if path.exists() {
        std::fs::remove_file(&path).map_err(|error| error.to_string())?;
    }
    std::fs::rename(&temp, &path).map_err(|error| error.to_string())
}

pub fn verify_installation(
    root: &Path,
    manifest: &PluginManifest,
    trusted: &HashMap<String, String>,
) -> Result<String, String> {
    ensure_no_native_files(root)?;
    let Some(integrity) = &manifest.integrity else {
        if manifest.signature.is_some() {
            return Err("签名插件必须提供 integrity 文件清单".into());
        }
        return Ok("unsigned".into());
    };
    if !integrity.algorithm.eq_ignore_ascii_case("sha256") {
        return Err(format!("不支持的完整性算法：{}", integrity.algorithm));
    }
    if integrity.files.is_empty() {
        return Err("integrity.files 不能为空".into());
    }
    let actual_files = package_files(root)?;
    let declared_files = integrity
        .files
        .keys()
        .cloned()
        .collect::<std::collections::HashSet<_>>();
    if actual_files != declared_files {
        let missing = actual_files
            .difference(&declared_files)
            .cloned()
            .collect::<Vec<_>>();
        let extra = declared_files
            .difference(&actual_files)
            .cloned()
            .collect::<Vec<_>>();
        return Err(format!(
            "完整性清单与插件文件不一致；未声明={missing:?}，不存在={extra:?}"
        ));
    }
    for (relative, expected) in &integrity.files {
        let path = safe_package_path(root, relative)?;
        if !path.is_file() {
            return Err(format!("完整性文件不存在：{relative}"));
        }
        let actual = sha256_file(&path)?;
        if !actual.eq_ignore_ascii_case(expected.trim()) {
            return Err(format!("插件文件哈希不匹配：{relative}"));
        }
    }
    let Some(signature) = &manifest.signature else {
        return Ok("integrity-only".into());
    };
    verify_signature(manifest, signature)?;
    Ok(match trusted.get(&signature.key_id) {
        Some(public_key) if public_key == &signature.public_key => "trusted",
        _ => "signed-untrusted",
    }
    .into())
}

fn verify_signature(
    manifest: &PluginManifest,
    signature: &PluginSignatureManifest,
) -> Result<(), String> {
    if !signature.algorithm.eq_ignore_ascii_case("ed25519") {
        return Err(format!("不支持的签名算法：{}", signature.algorithm));
    }
    let public = STANDARD
        .decode(signature.public_key.trim())
        .map_err(|error| format!("插件公钥不是合法 Base64：{error}"))?;
    let public: [u8; 32] = public
        .try_into()
        .map_err(|_| "Ed25519 公钥必须为 32 字节".to_string())?;
    let key = VerifyingKey::from_bytes(&public).map_err(|error| error.to_string())?;
    let raw = STANDARD
        .decode(signature.value.trim())
        .map_err(|error| format!("插件签名不是合法 Base64：{error}"))?;
    let signature = Signature::from_slice(&raw).map_err(|error| error.to_string())?;
    key.verify_strict(&signing_payload(manifest), &signature)
        .map_err(|_| "插件签名验证失败".to_string())
}

pub fn signing_payload(manifest: &PluginManifest) -> Vec<u8> {
    let mut signable = manifest.clone();
    if let Some(signature) = signable.signature.as_mut() {
        signature.value.clear();
    }
    let mut payload = b"sayit-plugin-signature-v1\n".to_vec();
    let value = serde_json::to_value(&signable).expect("plugin manifest is serializable");
    payload.extend(serde_json::to_vec(&value).expect("plugin manifest value is serializable"));
    payload
}

pub fn install_from_directory(
    app: &tauri::AppHandle,
    source: &Path,
    allow_unsigned: bool,
    trust_signing_key: bool,
) -> Result<InstallResult, String> {
    let source = source
        .canonicalize()
        .map_err(|error| format!("插件目录不存在：{error}"))?;
    if !source.is_dir() {
        return Err("插件安装源必须是目录".into());
    }
    let manifest = validate_plugin_dir(&source)?;
    ensure_no_native_files(&source)?;
    let mut trusted = load_trusted_keys(app)?;
    let trust = verify_installation(&source, &manifest, &trusted)?;
    match trust.as_str() {
        "unsigned" | "integrity-only" if !allow_unsigned => {
            return Err("插件未签名；只有明确允许未签名插件后才能安装".into())
        }
        "signed-untrusted" if !trust_signing_key => {
            return Err("插件签名有效，但签名密钥尚未受信任".into())
        }
        "signed-untrusted" => {
            let signature = manifest.signature.as_ref().expect("signature checked");
            trusted.insert(signature.key_id.clone(), signature.public_key.clone());
            save_trusted_keys(app, &trusted)?;
        }
        _ => {}
    }

    let plugins = plugins_dir(app)?;
    let target = plugins.join(&manifest.id);
    let stage = plugins.join(format!(".install-{}-{}", manifest.id, Uuid::new_v4()));
    if let Err(error) = copy_directory(&source, &stage) {
        let _ = std::fs::remove_dir_all(&stage);
        return Err(error);
    }
    let staged_manifest = match validate_plugin_dir(&stage).and_then(|manifest| {
        verify_installation(&stage, &manifest, &trusted)?;
        Ok(manifest)
    }) {
        Ok(manifest) => manifest,
        Err(error) => {
            let _ = std::fs::remove_dir_all(&stage);
            return Err(error);
        }
    };

    let mut previous_backup = None;
    let replaced_version = if target.exists() {
        let (current_id, current_version) = installed_identity(&target)?;
        if current_id != manifest.id {
            return Err("已安装插件目录与新插件 ID 不一致".into());
        }
        let backup = backup_path(app, &current_id, &current_version)?;
        std::fs::rename(&target, &backup).map_err(|error| error.to_string())?;
        previous_backup = Some(backup);
        Some(current_version)
    } else {
        None
    };
    if let Err(error) = std::fs::rename(&stage, &target) {
        let _ = std::fs::remove_dir_all(&stage);
        if let Some(backup) = previous_backup {
            let _ = std::fs::rename(backup, &target);
        }
        return Err(format!("启用新插件失败：{error}"));
    }
    Ok(InstallResult {
        plugin_id: manifest.id,
        version: manifest.version,
        trust: verify_installation(&target, &staged_manifest, &trusted)?,
        replaced_version,
    })
}

pub fn install_from_path(
    app: &tauri::AppHandle,
    source: &Path,
    allow_unsigned: bool,
    trust_signing_key: bool,
) -> Result<InstallResult, String> {
    let source = source
        .canonicalize()
        .map_err(|error| format!("插件包不存在：{error}"))?;
    if source.is_dir() {
        return install_from_directory(app, &source, allow_unsigned, trust_signing_key);
    }
    if !source.is_file()
        || !source
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case(SAYIT_PACKAGE_EXTENSION))
    {
        return Err(format!(
            "请选择 .{SAYIT_PACKAGE_EXTENSION} 说吧包或开发目录"
        ));
    }
    let plugins = plugins_dir(app)?;
    let extracted = plugins.join(format!(".archive-{}", Uuid::new_v4()));
    let result = extract_archive(&source, &extracted)
        .and_then(|_| dispatch_sayit_package(&extracted, app, allow_unsigned, trust_signing_key));
    let _ = std::fs::remove_dir_all(&extracted);
    result
}

fn dispatch_sayit_package(
    root: &Path,
    app: &tauri::AppHandle,
    allow_unsigned: bool,
    trust_signing_key: bool,
) -> Result<InstallResult, String> {
    let declaration_path = root.join(PACKAGE_DECLARATION_FILE);
    let declaration_text = std::fs::read_to_string(&declaration_path)
        .map_err(|_| format!("说吧包缺少 {PACKAGE_DECLARATION_FILE}"))?;
    let declaration: SayItPackageDeclaration = serde_json::from_str(&declaration_text)
        .map_err(|error| format!("说吧包声明格式错误：{error}"))?;
    if declaration.format_version != 1 {
        return Err(format!(
            "不支持的说吧包格式版本：{}",
            declaration.format_version
        ));
    }
    match declaration.kind.as_str() {
        "provider-plugin" if declaration.entry == "manifest.json" => {
            install_from_directory(app, root, allow_unsigned, trust_signing_key)
        }
        "provider-plugin" => Err("供应商插件包入口必须是 manifest.json".into()),
        kind => Err(format!("当前版本不支持的说吧包类型：{kind}")),
    }
}

fn extract_archive(archive_path: &Path, destination: &Path) -> Result<(), String> {
    let file = std::fs::File::open(archive_path).map_err(|error| error.to_string())?;
    let mut archive =
        ZipArchive::new(file).map_err(|error| format!("插件包不是有效 ZIP：{error}"))?;
    if archive.len() > MAX_ARCHIVE_ENTRIES {
        return Err(format!("插件包文件数量超过上限：{MAX_ARCHIVE_ENTRIES}"));
    }
    let declared_size = (0..archive.len()).try_fold(0_u64, |total, index| {
        let entry = archive.by_index(index).map_err(|error| error.to_string())?;
        total
            .checked_add(entry.size())
            .ok_or_else(|| "插件包解压大小溢出".to_string())
    })?;
    if declared_size > MAX_ARCHIVE_UNCOMPRESSED_BYTES {
        return Err("插件包解压后超过 512 MB 上限".into());
    }
    std::fs::create_dir_all(destination).map_err(|error| error.to_string())?;
    let mut paths = std::collections::HashSet::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|error| error.to_string())?;
        if entry.is_symlink() {
            return Err("插件包不能包含符号链接".into());
        }
        let relative = entry
            .enclosed_name()
            .ok_or_else(|| "插件包包含越界路径".to_string())?;
        if relative.as_os_str().is_empty() {
            return Err("插件包包含空路径".into());
        }
        let key = relative.to_string_lossy().replace('\\', "/");
        if !paths.insert(key.clone()) {
            return Err("插件包包含重复路径".into());
        }
        let target = destination.join(&relative);
        if entry.is_dir() {
            std::fs::create_dir_all(target).map_err(|error| error.to_string())?;
            continue;
        }
        let parent = target.parent().ok_or("插件包文件路径无效")?;
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        reject_native_extension(&relative)?;
        let mut prefix = [0_u8; 4];
        let prefix_len = entry.read(&mut prefix).map_err(|error| error.to_string())?;
        reject_native_magic(&prefix[..prefix_len], &key)?;
        let mut output = std::fs::File::create(target).map_err(|error| error.to_string())?;
        output
            .write_all(&prefix[..prefix_len])
            .map_err(|error| error.to_string())?;
        std::io::copy(&mut entry, &mut output).map_err(|error| error.to_string())?;
        output.flush().map_err(|error| error.to_string())?;
    }
    Ok(())
}

pub fn list_backups(app: &tauri::AppHandle) -> Result<Vec<PluginBackup>, String> {
    let root = backups_dir(app)?;
    let mut backups = Vec::new();
    for entry in std::fs::read_dir(root).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        if !entry
            .file_type()
            .map_err(|error| error.to_string())?
            .is_dir()
        {
            continue;
        }
        if let Ok((plugin_id, version)) = installed_identity(&entry.path()) {
            backups.push(PluginBackup {
                plugin_id,
                version,
                directory: entry.file_name().to_string_lossy().into_owned(),
                created_at_ms: entry
                    .metadata()
                    .and_then(|metadata| metadata.modified())
                    .ok()
                    .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|duration| duration.as_millis() as u64)
                    .unwrap_or(0),
            });
        }
    }
    backups.sort_by(|left, right| right.created_at_ms.cmp(&left.created_at_ms));
    Ok(backups)
}

pub fn rollback(app: &tauri::AppHandle, plugin_id: &str) -> Result<InstallResult, String> {
    let backup = list_backups(app)?
        .into_iter()
        .find(|backup| backup.plugin_id == plugin_id)
        .ok_or_else(|| format!("插件 {plugin_id} 没有可回滚版本"))?;
    let backup_root = backups_dir(app)?.join(&backup.directory);
    install_from_directory(app, &backup_root, true, false)
}

fn backups_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let root = app
        .path()
        .app_local_data_dir()
        .map_err(|error| error.to_string())?
        .join(BACKUPS_DIR);
    std::fs::create_dir_all(&root).map_err(|error| error.to_string())?;
    Ok(root)
}

fn backup_path(app: &tauri::AppHandle, id: &str, version: &str) -> Result<PathBuf, String> {
    Ok(backups_dir(app)?.join(format!(
        "{}--{}--{}--{}",
        id,
        version.replace(|character: char| !character.is_ascii_alphanumeric(), "-"),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or(0),
        Uuid::new_v4()
    )))
}

fn installed_identity(root: &Path) -> Result<(String, String), String> {
    let value: Value = serde_json::from_slice(
        &std::fs::read(root.join("manifest.json")).map_err(|error| error.to_string())?,
    )
    .map_err(|error| format!("已安装插件清单损坏：{error}"))?;
    let id = value
        .get("id")
        .and_then(Value::as_str)
        .ok_or("已安装插件缺少 id")?;
    let version = value
        .get("version")
        .and_then(Value::as_str)
        .ok_or("已安装插件缺少 version")?;
    Ok((id.to_string(), version.to_string()))
}

fn safe_package_path(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let relative = Path::new(relative);
    if relative.is_absolute()
        || relative.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(format!("完整性文件路径越界：{}", relative.display()));
    }
    Ok(root.join(relative))
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn ensure_no_native_files(root: &Path) -> Result<(), String> {
    fn visit(root: &Path, directory: &Path) -> Result<(), String> {
        for entry in std::fs::read_dir(directory).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            let kind = entry.file_type().map_err(|error| error.to_string())?;
            if kind.is_symlink() {
                return Err(format!(
                    "插件包不能包含符号链接：{}",
                    entry.path().display()
                ));
            }
            if kind.is_dir() {
                visit(root, &entry.path())?;
                continue;
            }
            if kind.is_file() {
                let path = entry.path();
                let relative = path.strip_prefix(root).map_err(|error| error.to_string())?;
                reject_native_extension(relative)?;
                let mut file =
                    std::fs::File::open(entry.path()).map_err(|error| error.to_string())?;
                let mut prefix = [0_u8; 4];
                let size = file.read(&mut prefix).map_err(|error| error.to_string())?;
                reject_native_magic(&prefix[..size], &relative.to_string_lossy())?;
            }
        }
        Ok(())
    }
    visit(root, root)
}

fn reject_native_extension(path: &Path) -> Result<(), String> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if matches!(
        extension.as_str(),
        "exe" | "dll" | "so" | "dylib" | "com" | "scr" | "msi" | "node"
    ) {
        return Err(format!(
            ".sayit 不能包含原生可执行文件或动态库：{}",
            path.display()
        ));
    }
    Ok(())
}

fn reject_native_magic(prefix: &[u8], path: &str) -> Result<(), String> {
    let is_pe = prefix.starts_with(b"MZ");
    let is_elf = prefix.starts_with(b"\x7fELF");
    let is_macho = matches!(
        prefix,
        [0xfe, 0xed, 0xfa, 0xce, ..]
            | [0xce, 0xfa, 0xed, 0xfe, ..]
            | [0xfe, 0xed, 0xfa, 0xcf, ..]
            | [0xcf, 0xfa, 0xed, 0xfe, ..]
            | [0xca, 0xfe, 0xba, 0xbe, ..]
            | [0xbe, 0xba, 0xfe, 0xca, ..]
    );
    if is_pe || is_elf || is_macho {
        return Err(format!(".sayit 检测到原生二进制文件：{path}"));
    }
    Ok(())
}

fn package_files(root: &Path) -> Result<std::collections::HashSet<String>, String> {
    fn visit(
        root: &Path,
        directory: &Path,
        files: &mut std::collections::HashSet<String>,
    ) -> Result<(), String> {
        for entry in std::fs::read_dir(directory).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            let kind = entry.file_type().map_err(|error| error.to_string())?;
            if kind.is_symlink() {
                return Err(format!(
                    "插件包不能包含符号链接：{}",
                    entry.path().display()
                ));
            }
            if kind.is_dir() {
                visit(root, &entry.path(), files)?;
            } else if kind.is_file() {
                let relative = entry
                    .path()
                    .strip_prefix(root)
                    .map_err(|error| error.to_string())?
                    .to_string_lossy()
                    .replace('\\', "/");
                if relative != "manifest.json" {
                    files.insert(relative);
                }
            }
        }
        Ok(())
    }
    let mut files = std::collections::HashSet::new();
    visit(root, root, &mut files)?;
    Ok(files)
}

fn copy_directory(source: &Path, target: &Path) -> Result<(), String> {
    std::fs::create_dir_all(target).map_err(|error| error.to_string())?;
    for entry in std::fs::read_dir(source).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let kind = entry.file_type().map_err(|error| error.to_string())?;
        if kind.is_symlink() {
            return Err(format!(
                "插件包不能包含符号链接：{}",
                entry.path().display()
            ));
        }
        let destination = target.join(entry.file_name());
        if kind.is_dir() {
            copy_directory(&entry.path(), &destination)?;
        } else if kind.is_file() {
            std::fs::copy(entry.path(), destination).map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use zip::{write::SimpleFileOptions, ZipWriter};

    #[test]
    fn rejects_native_executable_in_archive() {
        let root = std::env::temp_dir().join(format!("sayit-plugin-archive-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let archive_path = root.join("plugin.sayit");
        let mut writer = ZipWriter::new(std::fs::File::create(&archive_path).unwrap());
        writer
            .start_file(PACKAGE_DECLARATION_FILE, SimpleFileOptions::default())
            .unwrap();
        writer
            .write_all(br#"{"formatVersion":1,"kind":"provider-plugin","entry":"manifest.json"}"#)
            .unwrap();
        writer
            .start_file("manifest.json", SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"{}").unwrap();
        writer
            .start_file("bin/connector.exe", SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"connector").unwrap();
        writer.finish().unwrap();
        let extracted = root.join("extracted");
        assert!(extract_archive(&archive_path, &extracted).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_archive_path_escape() {
        let root = std::env::temp_dir().join(format!("sayit-plugin-archive-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let archive_path = root.join("malicious.sayit");
        let mut writer = ZipWriter::new(std::fs::File::create(&archive_path).unwrap());
        writer
            .start_file("../outside.txt", SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"nope").unwrap();
        writer.finish().unwrap();
        assert!(extract_archive(&archive_path, &root.join("extracted")).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn signing_payload_is_stable_across_hash_map_order() {
        let mut left: PluginManifest = serde_json::from_value(serde_json::json!({
            "apiVersion": 3,
            "id": "test", "name": "Test", "version": "1.0.0",
            "provider": {"id":"test","displayName":"Test","config":{}},
            "models": [],
            "runtime": {"entrypoint":"connector/index.js","hostApiVersion":1},
            "integrity": {"algorithm":"sha256","files":{"b":"02","a":"01"}}
        }))
        .unwrap();
        let mut right = left.clone();
        right.integrity.as_mut().unwrap().files =
            HashMap::from([("a".into(), "01".into()), ("b".into(), "02".into())]);
        assert_eq!(signing_payload(&left), signing_payload(&right));
        left.integrity
            .as_mut()
            .unwrap()
            .files
            .insert("c".into(), "03".into());
        assert_ne!(signing_payload(&left), signing_payload(&right));
    }

    #[test]
    fn signed_package_detects_tampering_and_trusts_pinned_key() {
        let root = std::env::temp_dir().join(format!("sayit-signed-plugin-{}", Uuid::new_v4()));
        std::fs::create_dir_all(root.join("connector")).unwrap();
        std::fs::write(
            root.join("connector/index.js"),
            b"export default () => ({})",
        )
        .unwrap();
        let mut manifest: PluginManifest = serde_json::from_value(serde_json::json!({
            "apiVersion": 3,
            "id": "signed-test", "name": "Signed Test", "version": "1.0.0",
            "provider": {"id":"signed-test","displayName":"Signed Test","capabilities":["asr"],"config":{}},
            "models": [{
                "id":"signed-live","label":"Signed Live","providerId":"signed-test",
                "category":"realtime","protocol":"plugin-realtime-v1",
                "supportsVocabulary":false,"supportsAlignmentTimestamps":false,
                "scenes":["dictationRealtime"],"isDefaultRealtime":false,"isDefaultFile":false
            }],
            "runtime": {"entrypoint":"connector/index.js","hostApiVersion":1},
            "integrity": {"algorithm":"sha256","files":{"connector/index.js": sha256_file(&root.join("connector/index.js")).unwrap()}},
            "signature": {"algorithm":"ed25519","keyId":"test-key","publicKey":"","value":""}
        })).unwrap();
        let signing = SigningKey::from_bytes(&[7_u8; 32]);
        let public = STANDARD.encode(signing.verifying_key().as_bytes());
        manifest.signature.as_mut().unwrap().public_key = public.clone();
        manifest.signature.as_mut().unwrap().value =
            STANDARD.encode(signing.sign(&signing_payload(&manifest)).to_bytes());
        let trusted = HashMap::from([("test-key".into(), public)]);
        assert_eq!(
            verify_installation(&root, &manifest, &trusted).unwrap(),
            "trusted"
        );
        std::fs::write(root.join("connector/index.js"), b"tampered").unwrap();
        assert!(verify_installation(&root, &manifest, &trusted).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_native_magic_without_executable_extension() {
        let root = std::env::temp_dir().join(format!("sayit-native-magic-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("connector.bin"), b"\x7fELFpayload").unwrap();
        assert!(ensure_no_native_files(&root).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
