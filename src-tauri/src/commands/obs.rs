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

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ObsConnectionRequest {
    pub(crate) host: String,
    pub(crate) port: u16,
    #[serde(default)]
    pub(crate) password: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ObsInstallRequest {
    #[serde(flatten)]
    pub(crate) connection: ObsConnectionRequest,
    pub(crate) scene_name: String,
    pub(crate) source_width: u32,
    pub(crate) source_height: u32,
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
    pub(crate) canvas_width: u32,
    pub(crate) canvas_height: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ObsSavedConnection {
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) has_password: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ObsOverlayLayoutRequest {
    pub(crate) display_mode: String,
    pub(crate) width_percent: f64,
    pub(crate) font_size_percent: f64,
    pub(crate) line_count: u32,
    pub(crate) translation_enabled: bool,
    pub(crate) translation_layout: String,
}

#[tauri::command]
pub(crate) fn get_obs_connection_settings(
    state: tauri::State<'_, RuntimeState>,
) -> Result<ObsSavedConnection, String> {
    let settings = state
        .obs_overlay_settings
        .lock()
        .map_err(|_| "OBS overlay settings lock failed".to_string())?;
    Ok(ObsSavedConnection {
        host: settings.obs_host.clone(),
        port: settings.obs_port,
        has_password: !settings.obs_password.is_empty(),
    })
}

#[tauri::command]
pub(crate) fn get_obs_password(state: tauri::State<'_, RuntimeState>) -> Result<String, String> {
    state
        .obs_overlay_settings
        .lock()
        .map_err(|_| "OBS overlay settings lock failed".to_string())
        .map(|settings| settings.obs_password.clone())
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
pub(crate) async fn sync_obs_overlay_layout(
    app: tauri::AppHandle,
    request: ObsOverlayLayoutRequest,
    state: tauri::State<'_, RuntimeState>,
) -> Result<(), String> {
    let mut settings = state
        .obs_overlay_settings
        .lock()
        .map_err(|_| "OBS overlay settings lock failed".to_string())?
        .clone();
    let Some(input_uuid) = settings
        .input_uuid
        .as_deref()
        .and_then(|value| Uuid::parse_str(value).ok())
    else {
        return Ok(());
    };
    let connection = ObsConnectionRequest {
        host: settings.obs_host.clone(),
        port: settings.obs_port,
        password: Some(settings.obs_password.clone()),
    };
    let client = connect(&connection).await?;
    let video = client.config().video_settings().await.map_err(obs_error)?;
    let (width, height) = overlay_source_dimensions(video.base_width, video.base_height, &request);
    settings.obs_canvas_width = video.base_width;
    settings.obs_canvas_height = video.base_height;
    client
        .inputs()
        .set_settings(SetSettings {
            input: InputId::Uuid(input_uuid),
            settings: &browser_source_settings(&overlay_url(&settings), width, height),
            overlay: Some(true),
        })
        .await
        .map_err(obs_error)?;
    save_obs_overlay_settings(&app, &state, settings)
}

#[tauri::command]
pub(crate) async fn connect_obs(
    app: tauri::AppHandle,
    request: ObsConnectionRequest,
    state: tauri::State<'_, RuntimeState>,
) -> Result<ObsConnectionStatus, String> {
    let request = resolve_connection_request(&state, request)?;
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
    let video = client.config().video_settings().await.map_err(obs_error)?;
    let status = ObsConnectionStatus {
        obs_version: version.obs_studio_version.to_string(),
        websocket_version: version.obs_web_socket_version.to_string(),
        scenes: scenes
            .into_iter()
            .map(|scene| ObsSceneInfo {
                name: scene.id.name,
            })
            .collect(),
        browser_source_available,
        canvas_width: video.base_width,
        canvas_height: video.base_height,
    };
    save_connection_settings(&app, &state, &request)?;
    Ok(status)
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
    let connection = resolve_connection_request(&state, request.connection.clone())?;
    let client = connect(&connection).await?;
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
    let video = client.config().video_settings().await.map_err(obs_error)?;
    settings.obs_canvas_width = video.base_width;
    settings.obs_canvas_height = video.base_height;
    let source_width = request.source_width.clamp(160, video.base_width.max(160));
    let source_height = request.source_height.clamp(72, video.base_height.max(72));
    let browser_settings = browser_source_settings(&overlay_url(&settings), source_width, source_height);
    let inputs = client.inputs().list(None).await.map_err(obs_error)?;
    let scene_item_id;
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
        let source_name = settings
            .source_name
            .as_deref()
            .ok_or_else(|| "已保存的 OBS 字幕源名称无效，请先卸载后重新安装。".to_string())?;
        scene_item_id = client
            .scene_items()
            .list(SceneId::Uuid(selected_scene_uuid))
            .await
            .map_err(obs_error)?
            .into_iter()
            .find(|item| item.source_name == source_name)
            .map(|item| item.id)
            .ok_or_else(|| "无法在目标场景中定位现有字幕源，请先卸载后重新安装。".to_string())?;
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
        scene_item_id = created.scene_item_id;
        settings.input_uuid = Some(created.input_uuid.to_string());
        settings.source_name = Some(source_name);
        settings.scene_uuid = Some(selected_scene_uuid.to_string());
    }
    client
        .scene_items()
        .set_transform(SetTransform {
            scene: SceneId::Uuid(selected_scene_uuid),
            item_id: scene_item_id,
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
    settings.obs_host = connection.host;
    settings.obs_port = connection.port;
    settings.obs_password = connection.password.unwrap_or_default();
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
    let request = resolve_connection_request(&state, request)?;
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
    settings.obs_host = request.host;
    settings.obs_port = request.port;
    settings.obs_password = request.password.unwrap_or_default();
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
        request
            .password
            .as_deref()
            .filter(|password| !password.is_empty()),
    )
    .await
    .map_err(|error| obs_connect_error(error, host, request.port))
}

fn resolve_connection_request(
    state: &tauri::State<'_, RuntimeState>,
    mut request: ObsConnectionRequest,
) -> Result<ObsConnectionRequest, String> {
    let settings = state
        .obs_overlay_settings
        .lock()
        .map_err(|_| "OBS overlay settings lock failed".to_string())?;
    if request.host.trim().is_empty() {
        request.host = settings.obs_host.clone();
    }
    if request.port == 0 {
        request.port = settings.obs_port;
    }
    if request.password.is_none() {
        request.password = Some(settings.obs_password.clone());
    }
    Ok(request)
}

fn save_connection_settings(
    app: &tauri::AppHandle,
    state: &tauri::State<'_, RuntimeState>,
    request: &ObsConnectionRequest,
) -> Result<(), String> {
    let mut settings = state
        .obs_overlay_settings
        .lock()
        .map_err(|_| "OBS overlay settings lock failed".to_string())?
        .clone();
    settings.obs_host = request.host.clone();
    settings.obs_port = request.port;
    settings.obs_password = request.password.clone().unwrap_or_default();
    save_obs_overlay_settings(app, state, settings)
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

fn browser_source_settings(url: &str, width: u32, height: u32) -> Value {
    json!({
        "url": url,
        "width": width.max(1),
        "height": height.max(1),
        "fps": 30,
        "shutdown": false,
        "restart_when_active": false,
        "css": "body { background-color: rgba(0, 0, 0, 0); margin: 0; overflow: hidden; }"
    })
}

fn overlay_source_dimensions(
    canvas_width: u32,
    canvas_height: u32,
    request: &ObsOverlayLayoutRequest,
) -> (u32, u32) {
    let width_percent = request.width_percent.clamp(20.0, 70.0);
    let font_percent = request.font_size_percent.clamp(1.5, 6.0);
    let lines = if request.display_mode == "scroll" {
        request.line_count.clamp(1, 4)
    } else {
        1
    };
    let rows = if request.translation_enabled && request.translation_layout == "bilingual" {
        2
    } else {
        1
    };
    let font_size = canvas_height as f64 * font_percent / 100.0 * 1.8;
    let width = (canvas_width as f64 * width_percent / 100.0).round() as u32;
    let height = (font_size * 1.38 * lines as f64 * rows as f64
        + 20.0 * rows as f64
        + if rows > 1 { 10.0 } else { 0.0 })
        .ceil() as u32;
    (
        width.clamp(160, canvas_width.max(160)),
        height.clamp(72, canvas_height.max(72)),
    )
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

    #[test]
    fn browser_source_uses_requested_dimensions() {
        let settings = browser_source_settings("http://localhost/overlay", 2560, 1440);
        assert_eq!(settings["width"], 2560);
        assert_eq!(settings["height"], 1440);
    }

    #[test]
    fn saved_connection_status_does_not_serialize_password() {
        let status = ObsSavedConnection {
            host: "127.0.0.1".into(),
            port: 4455,
            has_password: true,
        };
        let value = serde_json::to_value(status).unwrap();
        assert_eq!(value["hasPassword"], true);
        assert!(value.get("password").is_none());
    }

    #[test]
    fn overlay_dimensions_follow_mode_lines_and_translation_rows() {
        let mut request = ObsOverlayLayoutRequest {
            display_mode: "replace".into(),
            width_percent: 46.0,
            font_size_percent: 2.6,
            line_count: 4,
            translation_enabled: false,
            translation_layout: "bilingual".into(),
        };
        let replace = overlay_source_dimensions(1920, 1080, &request);
        request.display_mode = "scroll".into();
        let scroll = overlay_source_dimensions(1920, 1080, &request);
        request.translation_enabled = true;
        let bilingual = overlay_source_dimensions(1920, 1080, &request);
        assert_eq!(replace.0, 883);
        assert!(scroll.1 > replace.1);
        assert!(bilingual.1 > scroll.1);
    }
}
