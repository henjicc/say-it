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
    #[serde(default = "default_obs_canvas_width")]
    pub(crate) obs_canvas_width: u32,
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

fn default_obs_canvas_width() -> u32 {
    1920
}

fn default_obs_canvas_height() -> u32 {
    1080
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ObsOverlayStyle {
    pub(crate) display_mode: String,
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
    if settings.obs_canvas_width == 0 {
        settings.obs_canvas_width = default_obs_canvas_width();
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

/// 页面内容指纹。OBS 的 Browser Source 页面常驻运行：应用升级重启后，OBS 里仍是旧页面在
/// 自行重连、按旧逻辑渲染，永远不会主动重新加载。把指纹拼进 URL，页面代码一变 URL 就变，
/// 下次同步字幕源设置时 OBS 检测到 URL 变化会自动重载页面；同版本内 URL 稳定，不会反复刷新。
fn overlay_page_version() -> u64 {
    OVERLAY_PAGE
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
        })
}

pub(crate) fn overlay_url(settings: &ObsOverlaySettings) -> String {
    format!(
        "http://127.0.0.1:{}{}?token={}&canvasWidth={}&canvasHeight={}&v={:x}",
        settings.port,
        OBS_OVERLAY_PATH,
        settings.token,
        settings.obs_canvas_width,
        settings.obs_canvas_height,
        overlay_page_version()
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
html,body,#root{width:100%;height:100%;margin:0;overflow:hidden;background:transparent}body{font-family:"Microsoft YaHei",sans-serif}#root{box-sizing:border-box;display:flex;align-items:center;justify-content:center}.stack{width:100%;display:flex;flex-direction:column;align-items:center;gap:10px}.caption{box-sizing:border-box;width:100%;max-width:100%;overflow:hidden;padding:10px 22px;text-align:center;word-break:break-word;font-weight:600;line-height:1.38;text-shadow:0 2px 8px rgba(0,0,0,.66);box-shadow:inset 0 0 0 1px rgba(255,255,255,.1)}.caption.empty{display:none}.flow{position:relative;overflow:hidden}.content{position:absolute;left:0;right:0;top:0;white-space:pre-wrap;overflow-wrap:anywhere;transform:translate3d(0,0,0);transition:transform var(--motion-duration,120ms) var(--motion-easing,ease-out);will-change:transform}.caption.replace .content{right:auto;white-space:nowrap}.caption.no-motion .content{transition:none}.fresh{animation:fresh-in var(--fade-duration,180ms) var(--fade-easing,ease-out) both}@keyframes fresh-in{from{opacity:0}to{opacity:1}}
</style></head><body><div id="root"><div class="stack" id="stack"><div class="caption empty" id="original"><div class="flow"><div class="content"></div></div></div><div class="caption empty" id="translation"><div class="flow"><div class="content"></div></div></div></div></div><script>
const q=new URLSearchParams(location.search),token=q.get('token')||'',canvasWidth=Math.max(640,Number(q.get('canvasWidth'))||1920),canvasHeight=Math.max(360,Number(q.get('canvasHeight'))||1080),original=document.getElementById('original'),translation=document.getElementById('translation'),stack=document.getElementById('stack'),channelState=new WeakMap();let retry=500;
function visibleText(value,style){const mode=style.displayMode==='scroll'?'scroll':'replace',lines=Math.max(1,Number(style.lineCount)||1),parts=String(value||'').split('\n');return mode==='scroll'?parts.slice(-lines).join('\n'):(parts.at(-1)||'')}
function updateContent(content,text,fade){const previous=channelState.get(content)||'',limit=Math.min(previous.length,text.length);let prefix=0;while(prefix<limit&&previous[prefix]===text[prefix])prefix++;content.textContent='';if(prefix)content.appendChild(document.createTextNode(text.slice(0,prefix)));const suffix=text.slice(prefix);if(suffix){const node=document.createElement('span');if(fade&&suffix.length<=10)node.className='fresh';node.textContent=suffix;content.appendChild(node)}channelState.set(content,text)}
function paint(el,raw,style){const text=visibleText(raw,style),mode=style.displayMode==='scroll'?'scroll':'replace',flow=el.querySelector('.flow'),content=el.querySelector('.content'),percent=Number(style.fontSizePercent)||0,base=percent>0?canvasHeight*percent/100:(Number(style.fontSize)||28),fontSize=Math.max(18,Math.round(base*1.8)),lines=mode==='scroll'?Math.max(1,Number(style.lineCount)||1):1,changed=(channelState.get(content)||'')!==text;el.className='caption '+mode+(text?'':' empty')+(style.motionEnabled===false?' no-motion':'');const widthPct=Number(style.widthPercent)||0;el.style.width=widthPct>0?Math.round(canvasWidth*widthPct/100)+'px':'100%';el.style.fontFamily=style.fontFamily||'Microsoft YaHei';el.style.fontSize=fontSize+'px';el.style.color=style.textColor||'#fff';el.style.background=style.backgroundColor||'rgba(5,7,10,.72)';el.style.borderRadius=Math.max(0,Number(style.rounded)||18)+'px';el.style.setProperty('--motion-duration',(Number(style.motionDurationMs)||120)+'ms');el.style.setProperty('--motion-easing',style.motionEasing||'ease-out');el.style.setProperty('--fade-duration',(Number(style.fadeDurationMs)||180)+'ms');el.style.setProperty('--fade-easing',style.fadeEasing||'ease-out');flow.style.height=Math.round(fontSize*1.38*lines)+'px';if(changed)updateContent(content,text,style.fadeEnabled!==false);requestAnimationFrame(()=>{const overflow=mode==='replace'?content.scrollWidth-flow.clientWidth:content.scrollHeight-flow.clientHeight;content.style.transform=mode==='replace'?`translate3d(${overflow>0?-overflow:(flow.clientWidth-content.scrollWidth)/2}px,0,0)`:`translate3d(0,${-(overflow>0?overflow:0)}px,0)`})}
function render(data){const s=data.style||{},bilingual=s.translationEnabled&&s.translationLayout==='bilingual',translationOnly=s.translationEnabled&&s.translationLayout==='translationOnly',first=s.translationOrder==='translationFirst';paint(original,translationOnly?'':data.originalText,s);paint(translation,translationOnly?data.translationText:(bilingual?data.translationText:''),s);if(first&&translation.parentElement===stack)stack.insertBefore(translation,original);if(!first&&original.parentElement===stack)stack.insertBefore(original,translation)}
function connect(){const scheme=location.protocol==='https:'?'wss':'ws',ws=new WebSocket(scheme+'://'+location.host+'/obs/caption-stream?token='+encodeURIComponent(token));ws.onopen=()=>{retry=500};ws.onmessage=e=>{try{render(JSON.parse(e.data))}catch{}};ws.onclose=()=>{setTimeout(connect,retry);retry=Math.min(10000,retry*2)};ws.onerror=()=>ws.close()}connect();
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
            obs_canvas_width: 1920,
            obs_canvas_height: 1080,
            ..Default::default()
        };
        let url = overlay_url(&settings);
        assert!(url
            .starts_with("http://127.0.0.1:12345/obs/overlay?token=secret&canvasWidth=1920&canvasHeight=1080&v="));
        // 同一份页面内容的版本指纹必须稳定，否则每次同步都会触发 OBS 重载页面、字幕闪断。
        assert_eq!(url, overlay_url(&settings));
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
        assert_eq!(settings.obs_canvas_width, 1920);
        assert_eq!(settings.obs_canvas_height, 1080);
        assert!(settings.obs_password.is_empty());
    }
}
