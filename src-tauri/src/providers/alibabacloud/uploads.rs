use std::path::{Path, PathBuf};

use reqwest::multipart::{Form, Part};
use serde::Deserialize;
use serde_json::Value;

const UPLOAD_POLICY_URL: &str = "https://dashscope.aliyuncs.com/api/v1/uploads";

#[derive(Clone, Debug, Deserialize)]
pub struct UploadPolicy {
    pub policy: String,
    pub signature: String,
    pub upload_dir: String,
    pub upload_host: String,
    pub oss_access_key_id: String,
    #[serde(rename = "x_oss_object_acl")]
    pub x_oss_object_acl: String,
    #[serde(rename = "x_oss_forbid_overwrite")]
    pub x_oss_forbid_overwrite: String,
    pub max_file_size_mb: u64,
}

#[derive(Debug, Deserialize)]
struct UploadPolicyResponse {
    data: UploadPolicy,
}

pub async fn get_upload_policy(api_key: &str, model: &str) -> Result<UploadPolicy, String> {
    let client = reqwest::Client::new();
    let resp = client
        .get(UPLOAD_POLICY_URL)
        .query(&[("action", "getPolicy"), ("model", model.trim())])
        .header("Authorization", format!("Bearer {}", api_key.trim()))
        .send()
        .await
        .map_err(|e| format!("获取临时 OSS 上传凭证失败：{e}"))?;
    let status = resp.status();
    let text = resp
        .text()
        .await
        .map_err(|e| format!("读取临时 OSS 上传凭证响应失败：{e}"))?;

    if !status.is_success() {
        return Err(format!(
            "获取临时 OSS 上传凭证返回 {status}：{}",
            extract_error_message(&text)
        ));
    }

    let value: UploadPolicyResponse = serde_json::from_str(&text)
        .map_err(|e| format!("解析临时 OSS 上传凭证失败：{e}（{}）", truncate(&text, 200)))?;
    Ok(value.data)
}

pub async fn upload_file(
    policy: &UploadPolicy,
    file_path: impl AsRef<Path>,
) -> Result<String, String> {
    let file_path = file_path.as_ref();
    let metadata = std::fs::metadata(file_path)
        .map_err(|e| format!("读取待上传文件信息失败：{}（{e}）", file_path.display()))?;
    if !metadata.is_file() {
        return Err(format!("待上传路径不是文件：{}", file_path.display()));
    }

    let max_bytes = policy
        .max_file_size_mb
        .saturating_mul(1024)
        .saturating_mul(1024);
    if metadata.len() > max_bytes {
        return Err(format!(
            "文件超过临时 OSS 限制：当前 {:.2} MB，最大 {} MB",
            metadata.len() as f64 / 1024.0 / 1024.0,
            policy.max_file_size_mb
        ));
    }

    let file_name = sanitize_file_name(file_path)?;
    let key = build_object_key(&policy.upload_dir, &file_name);
    let file_part = Part::file(file_path)
        .await
        .map_err(|e| format!("打开待上传文件失败：{}（{e}）", file_path.display()))?
        .file_name(file_name);

    let form = Form::new()
        .text("OSSAccessKeyId", policy.oss_access_key_id.clone())
        .text("Signature", policy.signature.clone())
        .text("policy", policy.policy.clone())
        .text("x-oss-object-acl", policy.x_oss_object_acl.clone())
        .text(
            "x-oss-forbid-overwrite",
            policy.x_oss_forbid_overwrite.clone(),
        )
        .text("key", key.clone())
        .text("success_action_status", "200")
        .part("file", file_part);

    let client = reqwest::Client::new();
    let resp = client
        .post(&policy.upload_host)
        .multipart(form)
        .send()
        .await
        .map_err(|e| format!("上传文件到临时 OSS 失败：{e}"))?;
    let status = resp.status();
    let text = resp
        .text()
        .await
        .map_err(|e| format!("读取临时 OSS 上传响应失败：{e}"))?;
    if !status.is_success() {
        return Err(format!(
            "上传文件到临时 OSS 返回 {status}：{}",
            extract_error_message(&text)
        ));
    }

    Ok(format!("oss://{key}"))
}

pub async fn upload_for_model(
    api_key: &str,
    model: &str,
    file_path: impl AsRef<Path>,
) -> Result<String, String> {
    if api_key.trim().is_empty() {
        return Err("请先保存阿里云百炼 API Key".to_string());
    }
    if model.trim().is_empty() {
        return Err("录音识别模型不能为空".to_string());
    }
    let file_path: PathBuf = file_path.as_ref().to_path_buf();
    let policy = get_upload_policy(api_key, model).await?;
    upload_file(&policy, file_path).await
}

fn sanitize_file_name(file_path: &Path) -> Result<String, String> {
    let original = file_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("无法读取文件名：{}", file_path.display()))?;
    let extension = file_path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(sanitize_name_part)
        .filter(|ext| !ext.is_empty());
    let stem = file_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(sanitize_name_part)
        .filter(|stem| !stem.is_empty())
        .unwrap_or_else(|| "audio".to_string());
    let mut file_name = match extension {
        Some(ext) => format!("{stem}.{ext}"),
        None => sanitize_name_part(original),
    };
    if file_name.is_empty() || file_name == "." || file_name == ".." {
        file_name = "audio".to_string();
    }
    Ok(file_name)
}

fn sanitize_name_part(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut previous_was_sep = false;
    for ch in value.chars() {
        let is_allowed = ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_');
        if is_allowed {
            out.push(ch);
            previous_was_sep = false;
        } else if !previous_was_sep {
            out.push('_');
            previous_was_sep = true;
        }
    }
    out.trim_matches(['.', '_', '-', ' ']).to_string()
}

fn build_object_key(upload_dir: &str, file_name: &str) -> String {
    let dir = upload_dir.trim().trim_matches('/');
    if dir.is_empty() {
        file_name.to_string()
    } else {
        format!("{dir}/{file_name}")
    }
}

fn extract_error_message(text: &str) -> String {
    let fallback = truncate(text, 200);
    if text.trim().is_empty() {
        return "空响应".to_string();
    }
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return fallback;
    };
    value
        .get("message")
        .or_else(|| value.get("msg"))
        .or_else(|| value.pointer("/error/message"))
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .unwrap_or(fallback)
}

fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max_chars).collect();
    out.push('…');
    out
}
