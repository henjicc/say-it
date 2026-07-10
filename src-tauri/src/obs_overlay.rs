use axum::{
    extract::{
        ws::{Message as WsMessage, WebSocket, WebSocketUpgrade},
        Query, State,
    },
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use serde::{Deserialize, Serialize};
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
};
use tokio::sync::watch;
use uuid::Uuid;

use crate::{persistence::save_persisted_state, state::RuntimeState};

pub(crate) const OBS_OVERLAY_PORT: u16 = 57_321;
const OBS_OVERLAY_PATH: &str = "/obs/overlay";
const OBS_STREAM_PATH: &str = "/obs/caption-stream";

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ObsOverlaySettings {
    #[serde(default = "default_overlay_port")]
    pub(crate) port: u16,
    #[serde(default)]
    pub(crate) token: String,
    #[serde(default)]
    pub(crate) input_uuid: Option<String>,
    #[serde(default)]
    pub(crate) scene_uuid: Option<String>,
    #[serde(default)]
    pub(crate) source_name: Option<String>,
    #[serde(default = "default_obs_host")]
    pub(crate) obs_host: String,
    #[serde(default = "default_obs_port")]
    pub(crate) obs_port: u16,
    #[serde(default)]
    pub(crate) obs_password: String,
    #[serde(default = "default_obs_canvas_height")]
    pub(crate) obs_canvas_height: u32,
}

fn default_overlay_port() -> u16 {
    OBS_OVERLAY_PORT
}

fn default_obs_host() -> String {
    "127.0.0.1".to_string()
}

fn default_obs_port() -> u16 {
    4455
}

fn default_obs_canvas_height() -> u32 {
    1080
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ObsOverlayStyle {
    pub(crate) font_family: String,
    pub(crate) font_size: u32,
    pub(crate) font_size_percent: f64,
    pub(crate) line_count: u32,
    pub(crate) width_percent: f64,
    pub(crate) text_color: String,
    pub(crate) background_color: String,
    pub(crate) rounded: u32,
    pub(crate) motion_enabled: bool,
    pub(crate) motion_duration_ms: u32,
    pub(crate) motion_easing: String,
    pub(crate) fade_enabled: bool,
    pub(crate) fade_duration_ms: u32,
    pub(crate) fade_easing: String,
    pub(crate) translation_enabled: bool,
    pub(crate) translation_layout: String,
    pub(crate) translation_order: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ObsOverlaySnapshot {
    pub(crate) original_text: String,
    pub(crate) translation_text: String,
    pub(crate) style: ObsOverlayStyle,
}

pub(crate) struct ObsOverlayRuntime {
    pub(crate) snapshot_tx: watch::Sender<ObsOverlaySnapshot>,
    pub(crate) started: AtomicBool,
    pub(crate) error: Mutex<Option<String>>,
}

impl Default for ObsOverlayRuntime {
    fn default() -> Self {
        let (snapshot_tx, _) = watch::channel(ObsOverlaySnapshot::default());
        Self {
            snapshot_tx,
            started: AtomicBool::new(false),
            error: Mutex::new(None),
        }
    }
}

#[derive(Clone)]
struct OverlayServerState {
    token: String,
    snapshot_tx: watch::Sender<ObsOverlaySnapshot>,
}

#[derive(Deserialize)]
struct TokenQuery {
    token: Option<String>,
}

pub(crate) fn ensure_obs_overlay_settings(
    state: &tauri::State<'_, RuntimeState>,
) -> Result<bool, String> {
    let mut settings = state
        .obs_overlay_settings
        .lock()
        .map_err(|_| "OBS overlay settings lock failed".to_string())?;
    let mut changed = false;
    if settings.port == 0 {
        settings.port = OBS_OVERLAY_PORT;
        changed = true;
    }
    if settings.token.trim().is_empty() {
        settings.token = Uuid::new_v4().simple().to_string();
        changed = true;
    }
    if settings.obs_host.trim().is_empty() {
        settings.obs_host = default_obs_host();
        changed = true;
    }
    if settings.obs_port == 0 {
        settings.obs_port = default_obs_port();
        changed = true;
    }
    if settings.obs_canvas_height == 0 {
        settings.obs_canvas_height = default_obs_canvas_height();
        changed = true;
    }
    Ok(changed)
}

pub(crate) fn start_obs_overlay_server(
    state: &tauri::State<'_, RuntimeState>,
) -> Result<(), String> {
    if state.obs_overlay_runtime.started.load(Ordering::Relaxed) {
        return Ok(());
    }
    let settings = state
        .obs_overlay_settings
        .lock()
        .map_err(|_| "OBS overlay settings lock failed".to_string())?
        .clone();
    let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), settings.port);
    let std_listener = match std::net::TcpListener::bind(address) {
        Ok(listener) => listener,
        Err(error) => {
            let message = format!("本地 OBS 字幕服务无法监听 {address}：{error}");
            if let Ok(mut guard) = state.obs_overlay_runtime.error.lock() {
                *guard = Some(message.clone());
            }
            return Err(message);
        }
    };
    std_listener
        .set_nonblocking(true)
        .map_err(|error| format!("设置 OBS 字幕服务为非阻塞模式失败：{error}"))?;
    let server_state = OverlayServerState {
        token: settings.token,
        snapshot_tx: state.obs_overlay_runtime.snapshot_tx.clone(),
    };
    state
        .obs_overlay_runtime
        .started
        .store(true, Ordering::Relaxed);
    let app = Router::new()
        .route(OBS_OVERLAY_PATH, get(overlay_page))
        .route(OBS_STREAM_PATH, get(overlay_stream))
        .with_state(server_state);
    tauri::async_runtime::spawn(async move {
        // Tokio listener registration requires an active runtime context. Tauri's setup hook is
        // synchronous, so perform the conversion inside the task managed by Tauri's async runtime.
        let Ok(listener) = tokio::net::TcpListener::from_std(std_listener) else {
            return;
        };
        let _ = axum::serve(listener, app).await;
    });
    Ok(())
}

pub(crate) fn overlay_url(settings: &ObsOverlaySettings) -> String {
    format!(
        "http://127.0.0.1:{}{}?token={}&canvasHeight={}",
        settings.port, OBS_OVERLAY_PATH, settings.token, settings.obs_canvas_height
    )
}

pub(crate) fn overlay_status(state: &RuntimeState) -> Result<ObsOverlayStatus, String> {
    let settings = state
        .obs_overlay_settings
        .lock()
        .map_err(|_| "OBS overlay settings lock failed".to_string())?
        .clone();
    let error = state
        .obs_overlay_runtime
        .error
        .lock()
        .map_err(|_| "OBS overlay server status lock failed".to_string())?
        .clone();
    Ok(ObsOverlayStatus {
        ready: state.obs_overlay_runtime.started.load(Ordering::Relaxed) && error.is_none(),
        connected: state.obs_overlay_runtime.snapshot_tx.receiver_count() > 0,
        url: overlay_url(&settings),
        installed: settings.input_uuid.is_some() && settings.scene_uuid.is_some(),
        source_name: settings.source_name,
        error,
    })
}

pub(crate) fn publish_overlay_snapshot(state: &RuntimeState, snapshot: ObsOverlaySnapshot) {
    state.obs_overlay_runtime.snapshot_tx.send_replace(snapshot);
}

pub(crate) fn save_obs_overlay_settings(
    app: &tauri::AppHandle,
    state: &tauri::State<'_, RuntimeState>,
    settings: ObsOverlaySettings,
) -> Result<(), String> {
    {
        let mut guard = state
            .obs_overlay_settings
            .lock()
            .map_err(|_| "OBS overlay settings lock failed".to_string())?;
        *guard = settings;
    }
    save_persisted_state(app, state)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ObsOverlayStatus {
    pub(crate) ready: bool,
    pub(crate) connected: bool,
    pub(crate) url: String,
    pub(crate) installed: bool,
    pub(crate) source_name: Option<String>,
    pub(crate) error: Option<String>,
}

fn authorized(expected: &str, supplied: Option<&str>) -> bool {
    supplied.is_some_and(|token| token == expected)
}

async fn overlay_page(
    State(state): State<OverlayServerState>,
    Query(query): Query<TokenQuery>,
) -> Response {
    if !authorized(&state.token, query.token.as_deref()) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let mut response = OVERLAY_PAGE.into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

async fn overlay_stream(
    State(state): State<OverlayServerState>,
    Query(query): Query<TokenQuery>,
    ws: WebSocketUpgrade,
) -> Response {
    if !authorized(&state.token, query.token.as_deref()) {
        return StatusCode::NOT_FOUND.into_response();
    }
    ws.on_upgrade(move |socket| stream_overlay(socket, state.snapshot_tx.subscribe()))
}

async fn stream_overlay(mut socket: WebSocket, mut snapshots: watch::Receiver<ObsOverlaySnapshot>) {
    let initial = snapshots.borrow().clone();
    if send_snapshot(&mut socket, &initial).await.is_err() {
        return;
    }
    loop {
        tokio::select! {
            changed = snapshots.changed() => {
                let snapshot = snapshots.borrow().clone();
                if changed.is_err() || send_snapshot(&mut socket, &snapshot).await.is_err() {
                    return;
                }
            }
            incoming = socket.recv() => {
                match incoming {
                    Some(Ok(WsMessage::Close(_))) | None | Some(Err(_)) => return,
                    _ => {}
                }
            }
        }
    }
}

async fn send_snapshot(socket: &mut WebSocket, snapshot: &ObsOverlaySnapshot) -> Result<(), ()> {
    let value = serde_json::to_string(snapshot).map_err(|_| ())?;
    socket
        .send(WsMessage::Text(value.into()))
        .await
        .map_err(|_| ())
}

const OVERLAY_PAGE: &str = r#"<!doctype html>
<html lang="zh-CN"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><style>
html,body,#root{width:100%;height:100%;margin:0;overflow:hidden;background:transparent}body{font-family:"Microsoft YaHei",sans-serif}#root{box-sizing:border-box;display:flex;align-items:center;justify-content:center}.stack{width:100%;display:flex;flex-direction:column;align-items:center;gap:10px}.caption{box-sizing:border-box;max-width:100%;padding:10px 22px;text-align:center;white-space:pre-wrap;word-break:break-word;font-weight:600;line-height:1.38;transition:opacity 180ms ease-out,transform 120ms ease-out}.caption.empty{visibility:hidden}.caption.fade{animation:fade 180ms ease-out}.caption.motion{transform:translateY(0)}@keyframes fade{from{opacity:.18;transform:translateY(6px)}to{opacity:1;transform:translateY(0)}}
</style></head><body><div id="root"><div class="stack" id="stack"><div class="caption" id="original"></div><div class="caption" id="translation"></div></div></div><script>
const q=new URLSearchParams(location.search), token=q.get('token')||'', canvasHeight=Math.max(360,Number(q.get('canvasHeight'))||1080), original=document.getElementById('original'), translation=document.getElementById('translation'), stack=document.getElementById('stack'); let retry=500;
function paint(el,text,style){el.textContent=text||'';el.className='caption'+(text?'':' empty')+(style.fadeEnabled&&text?' fade':'')+(style.motionEnabled?' motion':'');el.style.width='100%';el.style.fontFamily=style.fontFamily||'Microsoft YaHei';const percent=Number(style.fontSizePercent)||0,base=percent>0?canvasHeight*percent/100:(Number(style.fontSize)||28);el.style.fontSize=Math.max(18,Math.round(base*1.8))+'px';el.style.color=style.textColor||'#fff';el.style.background=style.backgroundColor||'rgba(5,7,10,.72)';el.style.borderRadius=Math.max(0,Number(style.rounded)||18)+'px';el.style.transitionDuration=(style.motionEnabled?(Number(style.motionDurationMs)||120):0)+'ms';}
function render(data){const s=data.style||{}; const bilingual=s.translationEnabled&&s.translationLayout==='bilingual'; const translationOnly=s.translationEnabled&&s.translationLayout==='translationOnly'; const first=s.translationOrder==='translationFirst'; paint(original,translationOnly?'':data.originalText,s); paint(translation,translationOnly?data.translationText:(bilingual?data.translationText:''),s); if(first&&translation.parentElement===stack)stack.insertBefore(translation,original); if(!first&&original.parentElement===stack)stack.insertBefore(original,translation);}
function connect(){const scheme=location.protocol==='https:'?'wss':'ws';const ws=new WebSocket(scheme+'://'+location.host+'/obs/caption-stream?token='+encodeURIComponent(token));ws.onopen=()=>{retry=500};ws.onmessage=e=>{try{render(JSON.parse(e.data))}catch{}};ws.onclose=()=>{setTimeout(connect,retry);retry=Math.min(10000,retry*2)};ws.onerror=()=>ws.close()};connect();
</script></body></html>"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_validation_rejects_missing_or_wrong_tokens() {
        assert!(authorized("expected", Some("expected")));
        assert!(!authorized("expected", None));
        assert!(!authorized("expected", Some("wrong")));
    }

    #[test]
    fn overlay_url_is_loopback_and_tokenized() {
        let settings = ObsOverlaySettings {
            port: 12_345,
            token: "secret".into(),
            obs_canvas_height: 1080,
            ..Default::default()
        };
        assert_eq!(
            overlay_url(&settings),
            "http://127.0.0.1:12345/obs/overlay?token=secret&canvasHeight=1080"
        );
    }

    #[test]
    fn new_clients_receive_the_latest_caption_snapshot() {
        let runtime = ObsOverlayRuntime::default();
        runtime.snapshot_tx.send_replace(ObsOverlaySnapshot {
            original_text: "最新字幕".into(),
            ..Default::default()
        });
        let receiver = runtime.snapshot_tx.subscribe();
        assert_eq!(runtime.snapshot_tx.receiver_count(), 1);
        assert_eq!(receiver.borrow().original_text, "最新字幕");
    }

    #[test]
    fn legacy_overlay_settings_receive_obs_connection_defaults() {
        let settings: ObsOverlaySettings = serde_json::from_str(r#"{"port":57321}"#).unwrap();
        assert_eq!(settings.obs_host, "127.0.0.1");
        assert_eq!(settings.obs_port, 4455);
        assert_eq!(settings.obs_canvas_height, 1080);
        assert!(settings.obs_password.is_empty());
    }
}
