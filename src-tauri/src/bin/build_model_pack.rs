use std::collections::{BTreeMap, HashMap};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use ed25519_dalek::pkcs8::DecodePrivateKey;
use ed25519_dalek::{Signer, SigningKey};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use zip::write::SimpleFileOptions;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BuildDescriptor {
    manifest: Value,
    sources: HashMap<String, String>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 4 {
        return Err(
            "用法：cargo run --features model-pack-builder --bin build_model_pack -- <descriptor.json> <输出目录> <发布者key-id> <Ed25519 PKCS#8 PEM、32字节或Base64私钥文件>"
                .into(),
        );
    }
    let descriptor_path = canonical_file(&args[0])?;
    let output_dir = PathBuf::from(&args[1]);
    let key_id = args[2].trim();
    if key_id.is_empty() {
        return Err("发布者 key-id 不能为空".into());
    }
    let signing_key = load_signing_key(Path::new(&args[3]))?;
    let descriptor: BuildDescriptor = serde_json::from_slice(
        &std::fs::read(&descriptor_path).map_err(|error| error.to_string())?,
    )
    .map_err(|error| format!("模型包描述文件格式错误：{error}"))?;
    let descriptor_dir = descriptor_path.parent().ok_or("描述文件路径无效")?;
    validate_manifest_shape(&descriptor.manifest)?;
    validate_source_map(&descriptor)?;
    std::fs::create_dir_all(&output_dir).map_err(|error| error.to_string())?;

    let id = text_field(&descriptor.manifest, "id")?;
    let version = text_field(&descriptor.manifest, "version")?;
    let embedded = output_dir.join(format!("{id}-{version}-embedded.sayit"));
    let manifest_only = output_dir.join(format!("{id}-{version}-manifest.sayit"));
    build_archive(
        &descriptor,
        descriptor_dir,
        key_id,
        &signing_key,
        &embedded,
        true,
    )?;
    build_archive(
        &descriptor,
        descriptor_dir,
        key_id,
        &signing_key,
        &manifest_only,
        false,
    )?;
    println!("{}", embedded.display());
    println!("{}", manifest_only.display());
    Ok(())
}

fn build_archive(
    descriptor: &BuildDescriptor,
    descriptor_dir: &Path,
    key_id: &str,
    signing_key: &SigningKey,
    output_path: &Path,
    embedded: bool,
) -> Result<(), String> {
    let declaration = serde_json::to_vec_pretty(&json!({
        "formatVersion": 1,
        "kind": "model-pack",
        "entry": "manifest.json"
    }))
    .map_err(|error| error.to_string())?;
    let mut sources = BTreeMap::<String, PathBuf>::new();
    let mut integrity_files = serde_json::Map::new();
    integrity_files.insert(
        "sayit-package.json".into(),
        Value::String(sha256(&declaration)),
    );
    if embedded {
        for (path, source) in &descriptor.sources {
            let source = descriptor_dir.join(source);
            let hash = validate_source(&descriptor.manifest, path, &source)?;
            integrity_files.insert(path.clone(), Value::String(hash));
            sources.insert(path.clone(), source);
        }
    }

    let mut manifest = descriptor.manifest.clone();
    normalize_manifest_defaults(&mut manifest);
    manifest["integrity"] = json!({ "algorithm": "sha256", "files": integrity_files });
    manifest["signature"] = json!({
        "algorithm": "ed25519",
        "keyId": key_id,
        "publicKey": STANDARD.encode(signing_key.verifying_key().as_bytes()),
        "value": ""
    });
    let signature = signing_key.sign(&signing_payload(&manifest));
    manifest["signature"]["value"] = Value::String(STANDARD.encode(signature.to_bytes()));
    let manifest_bytes = serde_json::to_vec_pretty(&manifest).map_err(|error| error.to_string())?;

    let output = std::fs::File::create(output_path).map_err(|error| error.to_string())?;
    let mut zip = zip::ZipWriter::new(output);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    zip.start_file("manifest.json", options)
        .map_err(|error| error.to_string())?;
    zip.write_all(&manifest_bytes)
        .map_err(|error| error.to_string())?;
    zip.start_file("sayit-package.json", options)
        .map_err(|error| error.to_string())?;
    zip.write_all(&declaration)
        .map_err(|error| error.to_string())?;
    for (path, source) in sources {
        zip.start_file(path, options)
            .map_err(|error| error.to_string())?;
        let mut input = std::fs::File::open(&source).map_err(|error| error.to_string())?;
        std::io::copy(&mut input, &mut zip).map_err(|error| error.to_string())?;
    }
    zip.finish().map_err(|error| error.to_string())?;
    Ok(())
}

fn validate_manifest_shape(manifest: &Value) -> Result<(), String> {
    if manifest.get("apiVersion").and_then(Value::as_u64) != Some(4)
        || manifest.pointer("/runtime/kind").and_then(Value::as_str) != Some("model-pack")
        || manifest
            .get("modelPack")
            .and_then(Value::as_object)
            .is_none()
    {
        return Err("描述文件 manifest 必须是 API v4 model-pack".into());
    }
    if manifest.get("integrity").is_some() || manifest.get("signature").is_some() {
        return Err("描述文件不得预先包含 integrity 或 signature".into());
    }
    text_field(manifest, "id")?;
    text_field(manifest, "version")?;
    Ok(())
}

fn validate_source_map(descriptor: &BuildDescriptor) -> Result<(), String> {
    let files = descriptor
        .manifest
        .pointer("/modelPack/files")
        .and_then(Value::as_array)
        .ok_or("modelPack.files 必须是数组")?;
    for file in files {
        let path = file
            .get("path")
            .and_then(Value::as_str)
            .ok_or("modelPack.files.path 不能为空")?;
        if !descriptor.sources.contains_key(path) {
            return Err(format!("内嵌包缺少模型源文件映射：{path}"));
        }
    }
    if descriptor.sources.len() != files.len() {
        return Err("sources 必须与 modelPack.files 一一对应".into());
    }
    Ok(())
}

fn normalize_manifest_defaults(manifest: &mut Value) {
    let provider = &mut manifest["provider"];
    if provider.get("authKind").is_none() {
        provider["authKind"] = Value::String("custom".into());
    }
    if provider.get("capabilities").is_none() {
        provider["capabilities"] = json!(["asr"]);
    }
    if provider.get("config").is_none() {
        provider["config"] = json!({});
    }
    if provider.get("configFields").is_none() {
        provider["configFields"] = json!([]);
    }
    if provider.get("actions").is_none() {
        provider["actions"] = json!([]);
    }
    let runtime = &mut manifest["runtime"];
    if runtime.get("hostApiVersion").is_none() {
        runtime["hostApiVersion"] = Value::from(1);
    }
    if runtime.get("permissions").is_none() {
        runtime["permissions"] = json!([]);
    }
    if runtime.get("network").is_none() {
        runtime["network"] = json!({ "allowedHosts": [] });
    }
}

fn validate_source(manifest: &Value, path: &str, source: &Path) -> Result<String, String> {
    let entry = manifest
        .pointer("/modelPack/files")
        .and_then(Value::as_array)
        .and_then(|files| {
            files
                .iter()
                .find(|file| file.get("path").and_then(Value::as_str) == Some(path))
        })
        .ok_or_else(|| format!("sources 包含未在 modelPack.files 声明的路径：{path}"))?;
    let size = std::fs::metadata(source)
        .map_err(|error| format!("读取模型源文件失败 {}：{error}", source.display()))?
        .len();
    if entry.get("sizeBytes").and_then(Value::as_u64) != Some(size) {
        return Err(format!("模型源文件大小与清单不一致：{path}"));
    }
    let expected = entry
        .get("sha256")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let actual = sha256_file(source)?;
    if !actual.eq_ignore_ascii_case(expected) {
        return Err(format!("模型源文件 SHA256 与清单不一致：{path}"));
    }
    Ok(actual)
}

fn signing_payload(manifest: &Value) -> Vec<u8> {
    let mut signable = manifest.clone();
    signable["signature"]["value"] = Value::String(String::new());
    let canonical = canonical_json(signable);
    let mut payload = b"sayit-plugin-signature-v1\n".to_vec();
    payload.extend(serde_json::to_vec(&canonical).expect("manifest is serializable"));
    payload
}

fn canonical_json(value: Value) -> Value {
    match value {
        Value::Object(object) => Value::Object(
            object
                .into_iter()
                .map(|(key, value)| (key, canonical_json(value)))
                .collect::<BTreeMap<_, _>>()
                .into_iter()
                .collect(),
        ),
        Value::Array(values) => Value::Array(values.into_iter().map(canonical_json).collect()),
        value => value,
    }
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let mut input = std::fs::File::open(path)
        .map_err(|error| format!("读取模型源文件失败 {}：{error}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        let count = input.read(&mut buffer).map_err(|error| error.to_string())?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn canonical_file(value: &str) -> Result<PathBuf, String> {
    Path::new(value)
        .canonicalize()
        .map_err(|error| format!("文件不存在 {value}：{error}"))
}

fn text_field<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("manifest.{key} 不能为空"))
}

fn load_signing_key(path: &Path) -> Result<SigningKey, String> {
    let raw = std::fs::read(path).map_err(|error| format!("读取私钥失败：{error}"))?;
    if raw.starts_with(b"-----BEGIN PRIVATE KEY-----") {
        let pem =
            String::from_utf8(raw).map_err(|error| format!("私钥 PEM 不是 UTF-8：{error}"))?;
        return SigningKey::from_pkcs8_pem(&pem)
            .map_err(|error| format!("解析 Ed25519 PKCS#8 私钥失败：{error}"));
    }
    let bytes = if raw.len() == 32 {
        raw
    } else {
        STANDARD
            .decode(String::from_utf8_lossy(&raw).trim())
            .map_err(|error| format!("私钥文件必须是 32 字节或其 Base64：{error}"))?
    };
    let bytes: [u8; 32] = bytes
        .try_into()
        .map_err(|_| "Ed25519 私钥必须为 32 字节".to_string())?;
    Ok(SigningKey::from_bytes(&bytes))
}
