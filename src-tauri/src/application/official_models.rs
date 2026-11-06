//! 官方模型包目录：从 ModelScope 仓库读取 `catalog.json`，一键安装签名清单包并下载权重。
//! 新增官方模型包只需更新仓库里的目录，不用发版。
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::application::plugin_management::{download_provider_model_pack, install_provider_plugin};
use crate::providers::model_download;
use crate::providers::plugin::{plugin_staging_dir, safe_model_file_path, PluginRegistrySnapshot};
use crate::state::RuntimeState;

const REPO_BASE: &str = "https://modelscope.cn/models/henjicc/sayit-models/resolve/master";
const CATALOG_FORMAT_VERSION: u32 = 1;
const MAX_CATALOG_BYTES: usize = 256 * 1024;
const MAX_MANIFEST_PACK_BYTES: u64 = 1024 * 1024;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Catalog {
    format_version: u32,
    packs: Vec<OfficialModelPack>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OfficialModelPack {
    id: String,
    name: String,
    description: String,
    version: String,
    size_bytes: u64,
    manifest: CatalogFile,
}

/// 目录只给仓库内的相对路径，下载地址由宿主拼出，目录无法把安装引到别的站点。
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct CatalogFile {
    path: String,
    sha256: String,
    size_bytes: u64,
}

async fn fetch_catalog(client: &reqwest::Client) -> Result<Vec<OfficialModelPack>, String> {
    let response = client
        .get(format!("{REPO_BASE}/catalog.json"))
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|error| format!("读取官方模型目录失败：{error}"))?;
    let bytes = response
        .bytes()
        .await
        .map_err(|error| format!("读取官方模型目录失败：{error}"))?;
    if bytes.len() > MAX_CATALOG_BYTES {
        return Err("官方模型目录过大".into());
    }
    let catalog: Catalog =
        serde_json::from_slice(&bytes).map_err(|error| format!("官方模型目录格式错误：{error}"))?;
    if catalog.format_version != CATALOG_FORMAT_VERSION {
        return Err("官方模型目录格式已更新，请升级软件".into());
    }
    for pack in &catalog.packs {
        // 只允许普通文件名字符，百分号编码的 `..` 也就无法跳出本仓库。
        if !pack
            .manifest
            .path
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-/".contains(&byte))
        {
            return Err(format!("官方模型目录路径非法：{}", pack.manifest.path));
        }
        safe_model_file_path(&pack.manifest.path)?;
        if pack.manifest.size_bytes > MAX_MANIFEST_PACK_BYTES {
            return Err(format!("官方模型包清单过大：{}", pack.id));
        }
    }
    Ok(catalog.packs)
}

#[tauri::command]
pub(crate) async fn list_official_model_packs() -> Result<Vec<OfficialModelPack>, String> {
    fetch_catalog(&model_download::https_client()?).await
}

/// 下载并安装签名清单包（官方密钥已内置信任），再按清单下载权重。
#[tauri::command]
pub(crate) async fn install_official_model_pack(
    app: tauri::AppHandle,
    pack_id: String,
    state: tauri::State<'_, RuntimeState>,
) -> Result<PluginRegistrySnapshot, String> {
    let client = model_download::https_client()?;
    let pack = fetch_catalog(&client)
        .await?
        .into_iter()
        .find(|pack| pack.id == pack_id)
        .ok_or_else(|| format!("官方模型目录里没有 {pack_id}"))?;
    let bytes = client
        .get(format!("{REPO_BASE}/{}", pack.manifest.path))
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|error| format!("下载模型包清单失败：{error}"))?
        .bytes()
        .await
        .map_err(|error| format!("下载模型包清单失败：{error}"))?;
    if bytes.len() as u64 != pack.manifest.size_bytes
        || !format!("{:x}", Sha256::digest(&bytes)).eq_ignore_ascii_case(&pack.manifest.sha256)
    {
        return Err("模型包清单与官方目录不一致，请稍后重试".into());
    }
    let path = plugin_staging_dir(&app)?.join(format!(".official-{}.sayit", uuid::Uuid::new_v4()));
    std::fs::write(&path, &bytes).map_err(|error| error.to_string())?;
    let installed = install_provider_plugin(
        app.clone(),
        path.to_string_lossy().into_owned(),
        Some(pack.manifest.sha256.clone()),
        false,
        false,
        state.clone(),
    )
    .await;
    // 删不掉（如被杀毒软件占用）也不影响安装结果，暂存目录会在下次启动时清空。
    let _ = std::fs::remove_file(&path);
    let snapshot = installed?;
    if !snapshot
        .plugins
        .iter()
        .any(|plugin| plugin.id == pack.id && plugin.version == pack.version)
    {
        return Err("安装的模型包与官方目录的 ID 或版本不一致".into());
    }
    download_provider_model_pack(app, pack.id, state).await
}
