use crate::{
    obs_overlay::{
        overlay_status as read_overlay_status, overlay_url,
        publish_overlay_snapshot as publish_snapshot, save_obs_overlay_settings,
        ObsOverlaySnapshot, ObsOverlayStatus,
    },
    state::RuntimeState,
};
use obws::{
    common::Alignment,
    requests::{
        inputs::{Create as CreateInput, InputId, SetSettings},
        scene_items::{Position, SceneItemTransform, SetTransform},
        scenes::SceneId,
    },
    Client,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

const OBS_BROWSER_SOURCE_KIND: &str = "browser_source";
const OBS_SOURCE_BASE_NAME: &str = "说吧！实时字幕";
const OBS_OVERLAY_WIDTH: u32 = 1280;
const OBS_OVERLAY_HEIGHT: u32 = 360;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ObsConnectionRequest {
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) password: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ObsInstallRequest {
    #[serde(flatten)]
    pub(crate) connection: ObsConnectionRequest,
    pub(crate) scene_name: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ObsSceneInfo {
    pub(crate) name: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ObsConnectionStatus {
    pub(crate) obs_version: String,
    pub(crate) websocket_version: String,
    pub(crate) scenes: Vec<ObsSceneInfo>,
    pub(crate) browser_source_available: bool,
}

#[tauri::command]
pub(crate) fn get_obs_overlay_status(
    state: tauri::State<'_, RuntimeState>,
) -> Result<ObsOverlayStatus, String> {
    read_overlay_status(&state)
}

#[tauri::command]
pub(crate) fn publish_obs_overlay_snapshot(
    snapshot: ObsOverlaySnapshot,
    state: tauri::State<'_, RuntimeState>,
) {
    publish_snapshot(&state, snapshot);
}

#[tauri::command]
pub(crate) async fn connect_obs(
    request: ObsConnectionRequest,
) -> Result<ObsConnectionStatus, String> {
    let client = connect(&request).await?;
    let version = client.general().version().await.map_err(obs_error)?;
    if version.obs_studio_version.major < 28 {
        return Err(format!(
            "OBS {} 不支持内置 obs-websocket；请使用 OBS 28 或更新版本。",
            version.obs_studio_version
        ));
    }
    let browser_source_available = client
        .inputs()
        .list_kinds(true)
        .await
        .map_err(obs_error)?
        .iter()
        .any(|kind| kind == OBS_BROWSER_SOURCE_KIND);
    let scenes = client.scenes().list().await.map_err(obs_error)?.scenes;
    Ok(ObsConnectionStatus {
        obs_version: version.obs_studio_version.to_string(),
        websocket_version: version.obs_web_socket_version.to_string(),
        scenes: scenes
            .into_iter()
            .map(|scene| ObsSceneInfo {
                name: scene.id.name,
            })
            .collect(),
        browser_source_available,
    })
}

#[tauri::command]
pub(crate) async fn install_obs_overlay(
    app: tauri::AppHandle,
    request: ObsInstallRequest,
    state: tauri::State<'_, RuntimeState>,
) -> Result<ObsOverlayStatus, String> {
    let initial_status = read_overlay_status(&state)?;
    if !initial_status.ready {
        return Err(initial_status
            .error
            .unwrap_or_else(|| "本地 OBS 字幕服务尚未就绪。".to_string()));
    }
    let scene_name = request.scene_name.trim();
    if scene_name.is_empty() {
        return Err("请选择要安装字幕源的 OBS 场景。".to_string());
    }
    let client = connect(&request.connection).await?;
    ensure_obs_support(&client).await?;
    let selected_scene = client
        .scenes()
        .list()
        .await
        .map_err(obs_error)?
        .scenes
        .into_iter()
        .find(|scene| scene.id.name == scene_name)
        .ok_or_else(|| "所选 OBS 场景已不存在，请重新连接后再试。".to_string())?;
    let selected_scene_uuid = selected_scene.id.uuid;

    let mut settings = state
        .obs_overlay_settings
        .lock()
        .map_err(|_| "OBS overlay settings lock failed".to_string())?
        .clone();
    let browser_settings = browser_source_settings(&overlay_url(&settings));
    let inputs = client.inputs().list(None).await.map_err(obs_error)?;
    if let Some(input_uuid) = settings
        .input_uuid
        .as_deref()
        .and_then(|value| Uuid::parse_str(value).ok())
        .filter(|uuid| inputs.iter().any(|input| input.id.uuid == *uuid))
    {
        if settings.scene_uuid.as_deref() != Some(selected_scene_uuid.to_string().as_str()) {
            return Err(
                "现有字幕源安装在其他场景。请先卸载后再安装到新场景，避免误删用户素材。"
                    .to_string(),
            );
        }
        client
            .inputs()
            .set_settings(SetSettings {
                input: InputId::Uuid(input_uuid),
                settings: &browser_settings,
                overlay: Some(true),
            })
            .await
            .map_err(obs_error)?;
    } else {
        let source_name = unique_source_name(&inputs);
        let created = client
            .inputs()
            .create(CreateInput {
                scene: SceneId::Uuid(selected_scene_uuid),
                input: &source_name,
                kind: OBS_BROWSER_SOURCE_KIND,
                settings: Some(browser_settings),
                enabled: Some(true),
            })
            .await
            .map_err(obs_error)?;
        let video = client.config().video_settings().await.map_err(obs_error)?;
        client
            .scene_items()
            .set_transform(SetTransform {
                scene: SceneId::Uuid(selected_scene_uuid),
                item_id: created.scene_item_id,
                transform: SceneItemTransform {
                    alignment: Some(Alignment::CENTER | Alignment::BOTTOM),
                    position: Some(Position {
                        x: Some(video.base_width as f32 / 2.0),
                        y: Some((video.base_height as f32 - 48.0).max(0.0)),
                    }),
                    ..Default::default()
                },
            })
            .await
            .map_err(obs_error)?;
        settings.input_uuid = Some(created.input_uuid.to_string());
        settings.source_name = Some(source_name);
        settings.scene_uuid = Some(selected_scene_uuid.to_string());
    }
    save_obs_overlay_settings(&app, &state, settings)?;
    read_overlay_status(&state)
}

#[tauri::command]
pub(crate) async fn uninstall_obs_overlay(
    app: tauri::AppHandle,
    request: ObsConnectionRequest,
    state: tauri::State<'_, RuntimeState>,
) -> Result<ObsOverlayStatus, String> {
    let mut settings = state
        .obs_overlay_settings
        .lock()
        .map_err(|_| "OBS overlay settings lock failed".to_string())?
        .clone();
    let input_uuid = settings
        .input_uuid
        .as_deref()
        .ok_or_else(|| "没有可卸载的说吧！OBS 字幕源。".to_string())
        .and_then(|value| {
            Uuid::parse_str(value).map_err(|_| "已保存的 OBS 字幕源记录无效。".to_string())
        })?;
    let client = connect(&request).await?;
    let exists = client
        .inputs()
        .list(None)
        .await
        .map_err(obs_error)?
        .iter()
        .any(|input| input.id.uuid == input_uuid);
    if exists {
        client
            .inputs()
            .remove(InputId::Uuid(input_uuid))
            .await
            .map_err(obs_error)?;
    }
    settings.input_uuid = None;
    settings.scene_uuid = None;
    settings.source_name = None;
    save_obs_overlay_settings(&app, &state, settings)?;
    read_overlay_status(&state)
}

async fn connect(request: &ObsConnectionRequest) -> Result<Client, String> {
    let host = request.host.trim();
    if host.is_empty() {
        return Err("OBS 地址不能为空。".to_string());
    }
    if request.port == 0 {
        return Err("OBS WebSocket 端口必须在 1 到 65535 之间。".to_string());
    }
    Client::connect(
        host,
        request.port,
        (!request.password.is_empty()).then_some(request.password.as_str()),
    )
    .await
    .map_err(|error| obs_connect_error(error, host, request.port))
}

fn obs_connect_error(error: obws::error::Error, host: &str, port: u16) -> String {
    let endpoint = format!("ws://{host}:{port}");
    let detail = error_chain(&error);
    let guidance = match &error {
        obws::error::Error::Connect(_) => {
            "无法建立网络连接。请确认 OBS 已启动，并在“工具 → WebSocket 服务器设置”中启用 WebSocket 服务器，且端口与此处一致。"
        }
        obws::error::Error::Timeout => {
            "连接超时。请检查地址、端口、防火墙，以及 OBS WebSocket 服务器是否已启用。"
        }
        obws::error::Error::Handshake(_) => {
            "已连接到目标端口，但 OBS WebSocket 握手失败。请检查 WebSocket 密码，并确认该端口确实属于 OBS 28 或更新版本。"
        }
        obws::error::Error::InvalidUri(_) => "地址格式无效，请填写 IP 地址或主机名，不要包含 ws://、路径或端口。",
        obws::error::Error::ObsStudioVersion(_, _)
        | obws::error::Error::ObsWebsocketVersion(_, _) => {
            "OBS 或 obs-websocket 版本不兼容，请升级到 OBS 28 或更新版本。"
        }
        _ => "连接 OBS WebSocket 时发生异常。",
    };
    format!("无法连接 OBS WebSocket（{endpoint}）。{guidance}\n技术详情：{detail}")
}

fn error_chain(error: &(dyn std::error::Error + 'static)) -> String {
    let mut messages = vec![error.to_string()];
    let mut source = error.source();
    while let Some(current) = source {
        let message = current.to_string();
        if !messages.iter().any(|existing| existing == &message) {
            messages.push(message);
        }
        source = current.source();
    }
    messages.join(": ")
}

async fn ensure_obs_support(client: &Client) -> Result<(), String> {
    let version = client.general().version().await.map_err(obs_error)?;
    if version.obs_studio_version.major < 28 {
        return Err(format!(
            "OBS {} 不支持内置 obs-websocket；请使用 OBS 28 或更新版本。",
            version.obs_studio_version
        ));
    }
    let has_browser_source = client
        .inputs()
        .list_kinds(true)
        .await
        .map_err(obs_error)?
        .iter()
        .any(|kind| kind == OBS_BROWSER_SOURCE_KIND);
    if !has_browser_source {
        return Err("当前 OBS 未提供 Browser Source，无法安装实时字幕源。".to_string());
    }
    Ok(())
}

fn browser_source_settings(url: &str) -> Value {
    json!({
        "url": url,
        "width": OBS_OVERLAY_WIDTH,
        "height": OBS_OVERLAY_HEIGHT,
        "fps": 30,
        "shutdown": false,
        "restart_when_active": false,
        "css": "body { background-color: rgba(0, 0, 0, 0); margin: 0; overflow: hidden; }"
    })
}

fn unique_source_name(inputs: &[obws::responses::inputs::Input]) -> String {
    let exists = |name: &str| inputs.iter().any(|input| input.id.name == name);
    if !exists(OBS_SOURCE_BASE_NAME) {
        return OBS_SOURCE_BASE_NAME.to_string();
    }
    (2..)
        .map(|index| format!("{OBS_SOURCE_BASE_NAME} {index}"))
        .find(|name| !exists(name))
        .unwrap_or_else(|| format!("{OBS_SOURCE_BASE_NAME} {}", Uuid::new_v4().simple()))
}

fn obs_error(error: impl std::fmt::Display) -> String {
    format!("无法连接或操作 OBS：{error}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connect_timeout_includes_endpoint_and_actionable_guidance() {
        let message = obs_connect_error(obws::error::Error::Timeout, "127.0.0.1", 4455);
        assert!(message.contains("ws://127.0.0.1:4455"));
        assert!(message.contains("连接超时"));
        assert!(message.contains("防火墙"));
        assert!(message.contains("技术详情"));
    }
}
