use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use hmac::{Hmac, Mac};
use rand::RngCore;
use rquickjs::loader::{ImportAttributes, Loader, Resolver};
use rquickjs::{
    CaughtError, Context, Ctx, Error as JsError, Function, Module, Object, Promise, Runtime,
    TypedArray,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message};
use tokio_util::io::ReaderStream;
use url::Url;

use super::plugin::PluginRuntimeSpec;
use super::browser_session_capture::validate_capture_for_runtime;
use super::plugin_secrets;
use super::ProviderProfile;

pub const DEFAULT_INVOKE_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const MAX_MEMORY_BYTES: usize = 64 * 1024 * 1024;
const MAX_STACK_BYTES: usize = 1024 * 1024;
const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;
const MAX_EVENTS: usize = 1024;

#[derive(Clone)]
struct SafeResolver;

struct SafeLoader {
    root: PathBuf,
}

impl Resolver for SafeResolver {
    fn resolve<'js>(
        &mut self,
        _ctx: &Ctx<'js>,
        base: &str,
        name: &str,
        _attributes: Option<ImportAttributes<'js>>,
    ) -> rquickjs::Result<String> {
        if !name.starts_with("./") && !name.starts_with("../") {
            return Err(JsError::new_resolving_message(
                base,
                name,
                "只允许插件目录内的相对导入",
            ));
        }
        let base_dir = Path::new(base).parent().unwrap_or_else(|| Path::new(""));
        let mut normalized = PathBuf::new();
        for component in base_dir.join(name).components() {
            match component {
                Component::Normal(value) => normalized.push(value),
                Component::CurDir => {}
                Component::ParentDir => {
                    if !normalized.pop() {
                        return Err(JsError::new_resolving_message(
                            base,
                            name,
                            "相对导入不能跳出插件目录",
                        ));
                    }
                }
                _ => {
                    return Err(JsError::new_resolving_message(
                        base,
                        name,
                        "模块路径必须是相对路径",
                    ))
                }
            }
        }
        if normalized.extension().is_none() {
            normalized.set_extension("js");
        }
        Ok(normalized.to_string_lossy().replace('\\', "/"))
    }
}

impl Loader for SafeLoader {
    fn load<'js>(
        &mut self,
        ctx: &Ctx<'js>,
        name: &str,
        _attributes: Option<ImportAttributes<'js>>,
    ) -> rquickjs::Result<Module<'js, rquickjs::module::Declared>> {
        let relative = Path::new(name);
        if relative.is_absolute()
            || relative.components().any(|part| {
                matches!(
                    part,
                    Component::ParentDir | Component::RootDir | Component::Prefix(_)
                )
            })
        {
            return Err(JsError::new_loading_message(name, "非法模块路径"));
        }
        let path = self.root.join(relative);
        let canonical_root = self
            .root
            .canonicalize()
            .map_err(|error| JsError::new_loading_message(name, error.to_string()))?;
        let canonical_path = path
            .canonicalize()
            .map_err(|error| JsError::new_loading_message(name, error.to_string()))?;
        if !canonical_path.starts_with(canonical_root)
            || !matches!(
                canonical_path.extension().and_then(|value| value.to_str()),
                Some("js" | "mjs")
            )
        {
            return Err(JsError::new_loading_message(
                name,
                "模块必须是插件目录内的 JavaScript 文件",
            ));
        }
        let source = std::fs::read(&canonical_path)
            .map_err(|error| JsError::new_loading_message(name, error.to_string()))?;
        Module::declare(ctx.clone(), name, source)
    }
}

enum WsCommand {
    Send(Message),
    Close,
}

struct HostState {
    spec: PluginRuntimeSpec,
    inputs: HashMap<String, PathBuf>,
    events: Arc<Mutex<Vec<Value>>>,
    ws_connections: HashMap<String, mpsc::UnboundedSender<WsCommand>>,
    ws_events_tx: std::sync::mpsc::Sender<Value>,
    ws_events_rx: std::sync::mpsc::Receiver<Value>,
    cancelled: Arc<AtomicBool>,
    event_tx: Option<mpsc::Sender<Value>>,
    deadline: Arc<Mutex<Instant>>,
}

impl HostState {
    fn new(
        spec: PluginRuntimeSpec,
        inputs: HashMap<String, PathBuf>,
        events: Arc<Mutex<Vec<Value>>>,
        cancelled: Arc<AtomicBool>,
        event_tx: Option<mpsc::Sender<Value>>,
        deadline: Arc<Mutex<Instant>>,
    ) -> Self {
        let (ws_events_tx, ws_events_rx) = std::sync::mpsc::channel();
        Self {
            spec,
            inputs,
            events,
            ws_connections: HashMap::new(),
            ws_events_tx,
            ws_events_rx,
            cancelled,
            event_tx,
            deadline,
        }
    }

    fn call(&mut self, operation: &str, payload: Value) -> Result<Value, String> {
        match operation {
            "base64.encode" => {
                let bytes = payload_bytes(&payload)?;
                Ok(json!(
                    base64::engine::general_purpose::STANDARD.encode(bytes)
                ))
            }
            "base64.decode" => {
                let value = payload
                    .get("value")
                    .and_then(Value::as_str)
                    .ok_or("缺少 value")?;
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(value)
                    .map_err(|error| format!("Base64 解码失败：{error}"))?;
                Ok(json!(bytes))
            }
            "text.decodeUtf8" => String::from_utf8(payload_bytes(&payload)?)
                .map(Value::String)
                .map_err(|_| "UTF-8 解码失败".into()),
            "crypto.randomBytes" => {
                let size = payload
                    .get("size")
                    .and_then(Value::as_u64)
                    .unwrap_or(16)
                    .min(4096) as usize;
                let mut bytes = vec![0_u8; size];
                rand::rng().fill_bytes(&mut bytes);
                Ok(json!(bytes))
            }
            "crypto.sha256" => Ok(json!(hex_lower(&Sha256::digest(payload_bytes(&payload)?)))),
            "crypto.hmacSha256" => {
                let key = value_bytes(payload.get("key").ok_or("缺少 key")?)?;
                let data = value_bytes(payload.get("data").ok_or("缺少 data")?)?;
                let mut hmac =
                    Hmac::<Sha256>::new_from_slice(&key).map_err(|error| error.to_string())?;
                hmac.update(&data);
                Ok(json!(hex_lower(&hmac.finalize().into_bytes())))
            }
            "time.now" => Ok(json!(SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis())),
            "time.sleep" => {
                let millis = payload
                    .get("millis")
                    .and_then(Value::as_u64)
                    .unwrap_or_default()
                    .min(30_000);
                let started = Instant::now();
                while started.elapsed() < Duration::from_millis(millis) {
                    if self.cancelled.load(Ordering::Relaxed) {
                        return Err("插件操作已取消".into());
                    }
                    if deadline_expired(&self.deadline) {
                        return Err("插件操作超时".into());
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                Ok(Value::Null)
            }
            "storage.get" => {
                let key = storage_key(&payload)?;
                Ok(self.load_storage()?.remove(&key).unwrap_or(Value::Null))
            }
            "storage.set" => {
                let key = storage_key(&payload)?;
                let mut storage = self.load_storage()?;
                storage.insert(key, payload.get("value").cloned().unwrap_or(Value::Null));
                self.save_storage(&storage)?;
                Ok(Value::Null)
            }
            "storage.delete" => {
                let key = storage_key(&payload)?;
                let mut storage = self.load_storage()?;
                storage.remove(&key);
                self.save_storage(&storage)?;
                Ok(Value::Null)
            }
            "resource.readBytes" => Ok(json!(self.read_resource(&payload)?)),
            "resource.readText" => String::from_utf8(self.read_resource(&payload)?)
                .map(Value::String)
                .map_err(|_| "插件资源不是 UTF-8 文本".into()),
            "cancel.isCancelled" => Ok(json!(self.cancelled.load(Ordering::Relaxed))),
            "http.request" => self.http_request(payload),
            "websocket.open" => self.websocket_open(payload),
            "websocket.send" => self.websocket_send(payload),
            "websocket.close" => self.websocket_close(payload),
            "emit" => {
                let event = payload.get("event").cloned().unwrap_or(payload);
                if let Some(tx) = &self.event_tx {
                    tx.try_send(event)
                        .map_err(|_| "插件事件队列已满或接收端已关闭")?;
                    return Ok(Value::Null);
                }
                let mut events = self.events.lock().map_err(|_| "插件事件队列锁定失败")?;
                if events.len() >= MAX_EVENTS {
                    return Err("插件事件队列已满".into());
                }
                events.push(event);
                Ok(Value::Null)
            }
            "log" => {
                let level = payload
                    .get("level")
                    .and_then(Value::as_str)
                    .unwrap_or("info");
                let message = payload
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let line = format!(
                    "[plugin {}][{}] {}",
                    self.spec.plugin_id,
                    level,
                    redact(message)
                );
                crate::dlog!("{line}");
                Ok(Value::Null)
            }
            _ => Err(format!("未知宿主 API：{operation}")),
        }
    }

    fn take_ws_events(&self) -> Vec<Value> {
        let mut events = Vec::new();
        while events.len() < MAX_EVENTS {
            match self.ws_events_rx.try_recv() {
                Ok(event) => events.push(event),
                Err(_) => break,
            }
        }
        events
    }

    fn load_storage(&self) -> Result<serde_json::Map<String, Value>, String> {
        let path = self.spec.data_dir.join("storage.json");
        if !path.exists() {
            return Ok(serde_json::Map::new());
        }
        let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
        serde_json::from_slice(&bytes).map_err(|error| format!("插件存储损坏：{error}"))
    }

    fn read_resource(&self, payload: &Value) -> Result<Vec<u8>, String> {
        let relative = payload
            .get("path")
            .and_then(Value::as_str)
            .ok_or("资源读取缺少 path")?;
        let path = Path::new(relative);
        if path.is_absolute()
            || path.components().any(|part| {
                matches!(
                    part,
                    Component::ParentDir | Component::RootDir | Component::Prefix(_)
                )
            })
        {
            return Err("资源路径必须位于插件目录内".into());
        }
        let root = self
            .spec
            .root
            .canonicalize()
            .map_err(|error| error.to_string())?;
        let resource = self
            .spec
            .root
            .join(path)
            .canonicalize()
            .map_err(|error| error.to_string())?;
        if !resource.starts_with(root) || !resource.is_file() {
            return Err("插件资源不存在或越界".into());
        }
        let metadata = resource.metadata().map_err(|error| error.to_string())?;
        if metadata.len() > 1024 * 1024 {
            return Err("单个插件资源不能超过 1 MiB".into());
        }
        std::fs::read(resource).map_err(|error| error.to_string())
    }

    fn save_storage(&self, value: &serde_json::Map<String, Value>) -> Result<(), String> {
        std::fs::create_dir_all(&self.spec.data_dir).map_err(|error| error.to_string())?;
        let target = self.spec.data_dir.join("storage.json");
        let temporary = self.spec.data_dir.join("storage.json.tmp");
        std::fs::write(
            &temporary,
            serde_json::to_vec(value).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        std::fs::rename(temporary, target).map_err(|error| error.to_string())
    }

    fn http_request(&self, payload: Value) -> Result<Value, String> {
        require_network_permission(&self.spec)?;
        let method = payload
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or("GET")
            .to_uppercase();
        let mut url = parse_allowed_url(
            &self.spec,
            payload
                .get("url")
                .and_then(Value::as_str)
                .ok_or("HTTP 请求缺少 url")?,
            &["https"],
        )?;
        let headers = payload
            .get("headers")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        let body = request_body(&payload, &self.inputs)?;
        tauri::async_runtime::block_on(async {
            let client = reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .map_err(|error| error.to_string())?;
            for _ in 0..=5 {
                let mut request =
                    client.request(method.parse().map_err(|_| "非法 HTTP 方法")?, url.clone());
                for (name, value) in &headers {
                    if let Some(value) = value.as_str() {
                        request = request.header(name, value);
                    }
                }
                match &body {
                    Some(RequestBody::Bytes(body)) => request = request.body(body.clone()),
                    Some(RequestBody::Input(path)) => {
                        let file = tokio::fs::File::open(path)
                            .await
                            .map_err(|error| error.to_string())?;
                        request = request.body(reqwest::Body::wrap_stream(ReaderStream::new(file)));
                    }
                    None => {}
                }
                let response = tokio::select! {
                    response = request.send() => response.map_err(|error| error.to_string())?,
                    _ = wait_for_stop(self.cancelled.clone(), self.deadline.clone()) => return Err("插件操作已取消或超时".into()),
                };
                if response.status().is_redirection() {
                    let location = response
                        .headers()
                        .get(reqwest::header::LOCATION)
                        .ok_or("HTTP 重定向缺少 Location")?
                        .to_str()
                        .map_err(|error| error.to_string())?;
                    let next = url.join(location).map_err(|error| error.to_string())?;
                    url = parse_allowed_url(&self.spec, next.as_str(), &["https"])?;
                    continue;
                }
                let status = response.status().as_u16();
                let response_headers = response
                    .headers()
                    .iter()
                    .map(|(name, value)| {
                        (
                            name.to_string(),
                            Value::String(value.to_str().unwrap_or_default().to_string()),
                        )
                    })
                    .collect::<serde_json::Map<_, _>>();
                let mut stream = response.bytes_stream();
                let mut bytes = Vec::new();
                while let Some(chunk) = stream.next().await {
                    let chunk = chunk.map_err(|error| error.to_string())?;
                    if bytes.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
                        return Err("HTTP 响应超过 16 MiB 限制".into());
                    }
                    bytes.extend_from_slice(&chunk);
                }
                return Ok(json!({
                    "status": status,
                    "headers": response_headers,
                    "bodyText": String::from_utf8_lossy(&bytes),
                    "bodyBase64": base64::engine::general_purpose::STANDARD.encode(&bytes),
                }));
            }
            Err("HTTP 重定向次数过多".into())
        })
    }

    fn websocket_open(&mut self, payload: Value) -> Result<Value, String> {
        require_network_permission(&self.spec)?;
        let url = parse_allowed_url(
            &self.spec,
            payload
                .get("url")
                .and_then(Value::as_str)
                .ok_or("WebSocket 缺少 url")?,
            &["wss"],
        )?;
        let headers = payload
            .get("headers")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        let id = uuid::Uuid::new_v4().to_string();
        let (tx, mut rx) = mpsc::unbounded_channel();
        let events = self.ws_events_tx.clone();
        let event_id = id.clone();
        let cancelled = self.cancelled.clone();
        tauri::async_runtime::spawn(async move {
            let connection = (|| {
                let mut request = url
                    .as_str()
                    .into_client_request()
                    .map_err(|error| error.to_string())?;
                for (name, value) in headers {
                    let Some(value) = value.as_str() else {
                        continue;
                    };
                    let name = name
                        .parse::<tokio_tungstenite::tungstenite::http::HeaderName>()
                        .map_err(|error| error.to_string())?;
                    let value = value
                        .parse::<tokio_tungstenite::tungstenite::http::HeaderValue>()
                        .map_err(|error| error.to_string())?;
                    request.headers_mut().insert(name, value);
                }
                Ok::<_, String>(request)
            })();
            match connection {
                Ok(request) => match tokio_tungstenite::connect_async(request).await {
                    Ok((stream, _)) => {
                        let (mut writer, mut reader) = stream.split();
                        let _ =
                            events.send(json!({"type":"websocketOpen","connectionId":event_id}));
                        loop {
                            tokio::select! {
                                command = rx.recv() => match command {
                                    Some(WsCommand::Send(message)) => { if writer.send(message).await.is_err() { break; } }
                                    Some(WsCommand::Close) | None => { let _ = writer.close().await; break; }
                                },
                                incoming = reader.next() => match incoming {
                                    Some(Ok(Message::Text(text))) => { let _ = events.send(json!({"type":"websocketMessage","connectionId":event_id,"text":text.as_str()})); }
                                    Some(Ok(Message::Binary(bytes))) => { let _ = events.send(json!({"type":"websocketMessage","connectionId":event_id,"bytes":bytes.to_vec()})); }
                                    Some(Ok(Message::Close(_))) | None => break,
                                    Some(Ok(_)) => {}
                                    Some(Err(error)) => { let _ = events.send(json!({"type":"websocketError","connectionId":event_id,"message":error.to_string()})); break; }
                                },
                            _ = wait_for_cancel(cancelled.clone()) => break,
                            }
                        }
                    }
                    Err(error) => {
                        let _ = events.send(json!({"type":"websocketError","connectionId":event_id,"message":error.to_string()}));
                    }
                },
                Err(error) => {
                    let _ = events.send(
                        json!({"type":"websocketError","connectionId":event_id,"message":error}),
                    );
                }
            }
            let _ = events.send(json!({"type":"websocketClose","connectionId":event_id}));
        });
        self.ws_connections.insert(id.clone(), tx);
        Ok(json!({"connectionId": id}))
    }

    fn websocket_send(&self, payload: Value) -> Result<Value, String> {
        let id = payload
            .get("connectionId")
            .and_then(Value::as_str)
            .ok_or("缺少 connectionId")?;
        let tx = self.ws_connections.get(id).ok_or("WebSocket 连接不存在")?;
        let message = if let Some(text) = payload.get("text").and_then(Value::as_str) {
            Message::Text(text.to_string().into())
        } else {
            Message::Binary(value_bytes(payload.get("bytes").ok_or("缺少 text 或 bytes")?)?.into())
        };
        tx.send(WsCommand::Send(message))
            .map_err(|_| "WebSocket 已关闭")?;
        Ok(Value::Null)
    }

    fn websocket_close(&mut self, payload: Value) -> Result<Value, String> {
        let id = payload
            .get("connectionId")
            .and_then(Value::as_str)
            .ok_or("缺少 connectionId")?;
        if let Some(tx) = self.ws_connections.remove(id) {
            let _ = tx.send(WsCommand::Close);
        }
        Ok(Value::Null)
    }
}

impl Drop for HostState {
    fn drop(&mut self) {
        for (_, tx) in self.ws_connections.drain() {
            let _ = tx.send(WsCommand::Close);
        }
    }
}

pub struct JsProviderRuntime {
    _runtime: Runtime,
    context: Context,
    host: Arc<Mutex<HostState>>,
    events: Arc<Mutex<Vec<Value>>>,
    deadline: Arc<Mutex<Instant>>,
    cancelled: Arc<AtomicBool>,
}

impl JsProviderRuntime {
    pub fn create(
        spec: PluginRuntimeSpec,
        profile: &ProviderProfile,
        timeout: Duration,
        cancelled: Arc<AtomicBool>,
        inputs: HashMap<String, PathBuf>,
    ) -> Result<Self, String> {
        Self::create_with_event_sender(spec, profile, timeout, cancelled, inputs, None)
    }

    fn create_with_event_sender(
        spec: PluginRuntimeSpec,
        profile: &ProviderProfile,
        timeout: Duration,
        cancelled: Arc<AtomicBool>,
        inputs: HashMap<String, PathBuf>,
        event_tx: Option<mpsc::Sender<Value>>,
    ) -> Result<Self, String> {
        if spec.trust == "signed-untrusted" {
            return Err(format!(
                "插件 {} 的签名密钥尚未受信任，请从插件管理重新安装并确认发布者",
                spec.plugin_id
            ));
        }
        std::fs::create_dir_all(&spec.data_dir).map_err(|error| error.to_string())?;
        let runtime = Runtime::new().map_err(js_error)?;
        runtime.set_memory_limit(MAX_MEMORY_BYTES);
        runtime.set_max_stack_size(MAX_STACK_BYTES);
        runtime.set_loader(
            SafeResolver,
            SafeLoader {
                root: spec.root.clone(),
            },
        );
        let deadline = Arc::new(Mutex::new(Instant::now() + timeout));
        let interrupt_deadline = deadline.clone();
        let interrupt_cancelled = cancelled.clone();
        runtime.set_interrupt_handler(Some(Box::new(move || {
            interrupt_cancelled.load(Ordering::Relaxed)
                || interrupt_deadline
                    .lock()
                    .map(|value| Instant::now() >= *value)
                    .unwrap_or(true)
        })));
        let context = Context::full(&runtime).map_err(js_error)?;
        let events = Arc::new(Mutex::new(Vec::new()));
        let host = Arc::new(Mutex::new(HostState::new(
            spec.clone(),
            inputs,
            events.clone(),
            cancelled.clone(),
            event_tx,
            deadline.clone(),
        )));
        let session = plugin_secrets::load_session(&spec)?;
        if let Err(reason) = validate_capture_for_runtime(
            spec.browser_session.as_ref(),
            &session,
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|value| value.as_millis() as u64)
                .unwrap_or_default(),
        ) {
            return Err(format!(
                "浏览器临时会话凭据已失效：{reason}。请打开登录窗口并重新同步会话"
            ));
        }
        context.with(|ctx| -> Result<(), String> {
            let host_call_state = host.clone();
            let host_call = Function::new(
                ctx.clone(),
                move |operation: String, payload: String| -> String {
                    let parsed = serde_json::from_str(&payload).unwrap_or(Value::Null);
                    let result = host_call_state
                        .lock()
                        .map_err(|_| "宿主状态锁定失败".to_string())
                        .and_then(|mut state| state.call(&operation, parsed));
                    serde_json::to_string(&match result {
                        Ok(value) => json!({"ok":true,"value":value}),
                        Err(message) => json!({"ok":false,"error":message}),
                    })
                    .unwrap_or_else(|_| r#"{"ok":false,"error":"宿主返回序列化失败"}"#.into())
                },
            )
            .map_err(js_error)?;
            ctx.globals()
                .set("__sayitHostCall", host_call)
                .map_err(js_error)?;
            ctx.eval::<(), _>(HOST_BOOTSTRAP).map_err(js_error)?;
            let source = std::fs::read(&spec.entrypoint).map_err(|error| error.to_string())?;
            let entry_name = spec
                .entrypoint
                .strip_prefix(&spec.root)
                .map_err(|_| "插件入口不在插件目录内")?
                .to_string_lossy()
                .replace('\\', "/");
            let module = Module::declare(ctx.clone(), entry_name, source).map_err(js_error)?;
            let (module, promise) = module.eval().map_err(js_error)?;
            promise.finish::<()>().map_err(js_error)?;
            let factory: Function = module
                .get("default")
                .map_err(|_| "插件入口必须默认导出 createProvider(host) 函数".to_string())?;
            let host_object: Object = ctx.globals().get("__sayitHost").map_err(js_error)?;
            let provider: Object = factory.call((host_object,)).map_err(js_error)?;
            ctx.globals()
                .set("__sayitProvider", provider)
                .map_err(js_error)?;
            let init: Function = ctx.globals().get("__sayitInitialize").map_err(js_error)?;
            let request = json!({
                "providerId": profile.id,
                "config": profile.config,
                "session": session,
                "permissions": spec.permissions,
            });
            let promise: Promise = init
                .call((serde_json::to_string(&request).map_err(|error| error.to_string())?,))
                .map_err(|error| js_error_with_context(&ctx, error))?;
            promise
                .finish::<String>()
                .map_err(|error| js_error_with_context(&ctx, error))?;
            Ok(())
        })?;
        Ok(Self {
            _runtime: runtime,
            context,
            host,
            events,
            deadline,
            cancelled,
        })
    }

    pub fn call(&self, method: &str, payload: &Value, timeout: Duration) -> Result<Value, String> {
        *self.deadline.lock().map_err(|_| "插件截止时间锁定失败")? = Instant::now() + timeout;
        if self.cancelled.load(Ordering::Relaxed) {
            return Err("插件操作已取消".into());
        }
        self.context.with(|ctx| {
            let call: Function = ctx.globals().get("__sayitInvoke").map_err(js_error)?;
            let promise: Promise = call
                .call((
                    method,
                    serde_json::to_string(payload).map_err(|error| error.to_string())?,
                ))
                .map_err(|error| js_error_with_context(&ctx, error))?;
            let result = self.finish_promise_with_host_events(&ctx, promise)?;
            serde_json::from_str(&result)
                .map_err(|error| format!("插件返回值不是合法 JSON：{error}"))
        })
    }

    pub fn call_audio(&self, bytes: Vec<u8>) -> Result<(), String> {
        *self.deadline.lock().map_err(|_| "插件截止时间锁定失败")? =
            Instant::now() + Duration::from_secs(5);
        self.context.with(|ctx| {
            let call: Function = ctx.globals().get("__sayitAudio").map_err(js_error)?;
            let audio = TypedArray::<u8>::new(ctx.clone(), bytes).map_err(js_error)?;
            let promise: Promise = call
                .call((audio,))
                .map_err(|error| js_error_with_context(&ctx, error))?;
            self.finish_promise_with_host_events(&ctx, promise)?;
            Ok(())
        })
    }

    fn finish_promise_with_host_events<'js>(
        &self,
        ctx: &Ctx<'js>,
        promise: Promise<'js>,
    ) -> Result<String, String> {
        loop {
            if let Some(result) = promise.result::<String>() {
                return result.map_err(|error| js_error_with_context(ctx, error));
            }
            let mut progressed = false;
            while ctx.execute_pending_job() {
                progressed = true;
            }
            let events = self
                .host
                .lock()
                .map_err(|_| "宿主状态锁定失败")?
                .take_ws_events();
            for event in events {
                progressed = true;
                let call: Function = ctx.globals().get("__sayitInvoke").map_err(js_error)?;
                let callback: Promise = call
                    .call((
                        "onHostEvent",
                        serde_json::to_string(&event).map_err(|error| error.to_string())?,
                    ))
                    .map_err(|error| js_error_with_context(ctx, error))?;
                callback
                    .finish::<String>()
                    .map_err(|error| js_error_with_context(ctx, error))?;
            }
            if self.cancelled.load(Ordering::Relaxed) {
                return Err("插件操作已取消".into());
            }
            if self
                .deadline
                .lock()
                .map(|deadline| Instant::now() >= *deadline)
                .unwrap_or(true)
            {
                return Err("插件操作超时".into());
            }
            if !progressed {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }

    pub fn dispatch_host_events(&self) -> Result<(), String> {
        let events = self
            .host
            .lock()
            .map_err(|_| "宿主状态锁定失败")?
            .take_ws_events();
        for event in events {
            self.call("onHostEvent", &event, Duration::from_secs(5))?;
        }
        Ok(())
    }

    pub fn take_events(&self) -> Vec<Value> {
        self.events
            .lock()
            .map(|mut events| events.drain(..).collect())
            .unwrap_or_default()
    }
}

pub async fn invoke<F>(
    spec: &PluginRuntimeSpec,
    profile: &ProviderProfile,
    operation: &str,
    payload: Value,
    timeout: Duration,
    on_event: F,
) -> Result<Value, String>
where
    F: FnMut(&Value) + Send,
{
    invoke_cancellable(spec, profile, operation, payload, timeout, None, on_event).await
}

pub async fn invoke_cancellable<F>(
    spec: &PluginRuntimeSpec,
    profile: &ProviderProfile,
    operation: &str,
    mut payload: Value,
    timeout: Duration,
    cancel: Option<Arc<AtomicBool>>,
    mut on_event: F,
) -> Result<Value, String>
where
    F: FnMut(&Value) + Send,
{
    let spec = spec.clone();
    let profile = profile.clone();
    let operation = operation.to_string();
    let cancelled = cancel.unwrap_or_else(|| Arc::new(AtomicBool::new(false)));
    let (inputs, input_descriptor) = take_input_handle(&mut payload)?;
    if let Some(input) = input_descriptor {
        if let Some(object) = payload.as_object_mut() {
            object.insert("input".into(), input);
        }
    }
    let (event_tx, mut event_rx) = mpsc::channel(MAX_EVENTS);
    let mut task = tokio::task::spawn_blocking(move || {
        let runtime = JsProviderRuntime::create_with_event_sender(
            spec,
            &profile,
            timeout,
            cancelled,
            inputs,
            Some(event_tx),
        )?;
        let result = runtime.call(
            "invoke",
            &json!({"operation":operation,"payload":payload}),
            timeout,
        )?;
        runtime.dispatch_host_events()?;
        Ok::<_, String>(result)
    });
    loop {
        tokio::select! {
            result = &mut task => {
                while let Ok(event) = event_rx.try_recv() {
                    on_event(&event);
                }
                return result.map_err(|error| format!("插件运行线程失败：{error}"))?;
            }
            Some(event) = event_rx.recv() => on_event(&event),
        }
    }
}

fn take_input_handle(
    payload: &mut Value,
) -> Result<(HashMap<String, PathBuf>, Option<Value>), String> {
    let Some(path) = payload
        .as_object_mut()
        .and_then(|object| object.remove("filePath"))
        .and_then(|value| value.as_str().map(ToString::to_string))
    else {
        return Ok((HashMap::new(), None));
    };
    let metadata =
        std::fs::metadata(&path).map_err(|error| format!("读取插件输入失败：{error}"))?;
    if !metadata.is_file() {
        return Err("插件输入不是普通文件".into());
    }
    let id = uuid::Uuid::new_v4().to_string();
    let descriptor = json!({"id":id,"size":metadata.len(),"name":Path::new(&path).file_name().and_then(|value| value.to_str()).unwrap_or("audio")});
    Ok((HashMap::from([(id, PathBuf::from(path))]), Some(descriptor)))
}

/// 回环主机按字面精确匹配，不做 DNS 解析，防止自定义域名解析到 127.0.0.1 绕过白名单；
/// `localhost` 是系统保留名，视作唯一例外。IPv6 回环经 Url 解析后带方括号。
const LOOPBACK_HOSTS: &[&str] = &["127.0.0.1", "localhost", "[::1]"];

fn plaintext_scheme(secure: &str) -> Option<&'static str> {
    match secure {
        "https" => Some("http"),
        "wss" => Some("ws"),
        _ => None,
    }
}

fn parse_allowed_url(spec: &PluginRuntimeSpec, raw: &str, schemes: &[&str]) -> Result<Url, String> {
    let url = Url::parse(raw).map_err(|error| format!("非法网络地址：{error}"))?;
    let scheme = url.scheme();
    let is_plaintext_variant = schemes
        .iter()
        .any(|secure| plaintext_scheme(secure) == Some(scheme));
    if !schemes.contains(&scheme) && !is_plaintext_variant {
        return Err(format!("不允许的网络协议：{scheme}"));
    }
    let host = url
        .host_str()
        .ok_or("网络地址缺少主机名")?
        .to_ascii_lowercase();
    let local_permitted = LOOPBACK_HOSTS.contains(&host.as_str())
        && spec.permissions.iter().any(|value| value == "localNetwork");
    if !schemes.contains(&scheme) && !local_permitted {
        // 明文 http/ws 只对声明 localNetwork 权限的插件的回环地址放行。
        return Err(format!("不允许的网络协议：{scheme}"));
    }
    if local_permitted {
        // 本机端口由用户自己掌控，回环主机不要求出现在 allowedHosts。
        return Ok(url);
    }
    let allowed = spec.allowed_hosts.iter().any(|rule| {
        let rule = rule.to_ascii_lowercase();
        rule.strip_prefix("*.")
            .map(|suffix| host.ends_with(&format!(".{suffix}")) && host != suffix)
            .unwrap_or(host == rule)
    });
    if !allowed {
        return Err(format!("插件无权访问主机：{host}"));
    }
    Ok(url)
}

fn deadline_expired(deadline: &Arc<Mutex<Instant>>) -> bool {
    deadline
        .lock()
        .map(|deadline| Instant::now() >= *deadline)
        .unwrap_or(true)
}

async fn wait_for_stop(cancelled: Arc<AtomicBool>, deadline: Arc<Mutex<Instant>>) {
    while !cancelled.load(Ordering::Relaxed) && !deadline_expired(&deadline) {
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

async fn wait_for_cancel(cancelled: Arc<AtomicBool>) {
    while !cancelled.load(Ordering::Relaxed) {
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

/// 网络类宿主 API 的准入：`network`（外部 HTTPS/WSS）或 `localNetwork`（回环明文）任一即可，
/// 具体主机与协议限制由 `parse_allowed_url` 决定。
fn require_network_permission(spec: &PluginRuntimeSpec) -> Result<(), String> {
    if spec
        .permissions
        .iter()
        .any(|value| value == "network" || value == "localNetwork")
    {
        Ok(())
    } else {
        Err("插件未声明 network 权限".into())
    }
}

#[derive(Clone)]
enum RequestBody {
    Bytes(Vec<u8>),
    Input(PathBuf),
}

fn request_body(
    payload: &Value,
    inputs: &HashMap<String, PathBuf>,
) -> Result<Option<RequestBody>, String> {
    if let Some(id) = payload.get("inputId").and_then(Value::as_str) {
        let path = inputs.get(id).ok_or("无效或过期的输入句柄")?;
        return Ok(Some(RequestBody::Input(path.clone())));
    }
    if let Some(value) = payload.get("bodyBase64").and_then(Value::as_str) {
        return base64::engine::general_purpose::STANDARD
            .decode(value)
            .map(|value| Some(RequestBody::Bytes(value)))
            .map_err(|error| error.to_string());
    }
    Ok(payload
        .get("bodyText")
        .and_then(Value::as_str)
        .map(|value| RequestBody::Bytes(value.as_bytes().to_vec())))
}

fn payload_bytes(payload: &Value) -> Result<Vec<u8>, String> {
    value_bytes(payload.get("value").unwrap_or(payload))
}

fn value_bytes(value: &Value) -> Result<Vec<u8>, String> {
    if let Some(value) = value.as_str() {
        return Ok(value.as_bytes().to_vec());
    }
    let array = value.as_array().ok_or("值必须是字符串或字节数组")?;
    array
        .iter()
        .map(|value| {
            value
                .as_u64()
                .filter(|value| *value <= 255)
                .map(|value| value as u8)
                .ok_or_else(|| "字节数组包含非法值".to_string())
        })
        .collect()
}

fn storage_key(payload: &Value) -> Result<String, String> {
    let key = payload
        .get("key")
        .and_then(Value::as_str)
        .ok_or("存储操作缺少 key")?;
    if key.is_empty()
        || key.len() > 128
        || !key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
    {
        return Err("存储 key 只能包含 ASCII 字母、数字、点、横线和下划线".into());
    }
    Ok(key.to_string())
}

fn hex_lower(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn redact(message: &str) -> String {
    let mut result = message.to_string();
    for marker in ["authorization", "cookie", "token", "secret", "password"] {
        if result.to_ascii_lowercase().contains(marker) {
            result = "[日志包含敏感字段，已脱敏]".into();
            break;
        }
    }
    result
}

fn js_error(error: impl std::fmt::Display) -> String {
    format!("JavaScript 插件执行失败：{error}")
}

fn js_error_with_context(ctx: &Ctx<'_>, error: JsError) -> String {
    format!(
        "JavaScript 插件执行失败：{}",
        CaughtError::from_error(ctx, error)
    )
}

const HOST_BOOTSTRAP: &str = r#"
(() => {
  const call = (operation, payload = {}) => {
    const response = JSON.parse(globalThis.__sayitHostCall(operation, JSON.stringify(payload)));
    if (!response.ok) throw new Error(response.error || `宿主调用失败：${operation}`);
    return response.value;
  };
  const bytes = value => Array.from(value instanceof Uint8Array ? value : new Uint8Array(value));
  globalThis.__sayitHost = Object.freeze({
    http: Object.freeze({ request: options => call('http.request', options) }),
    websocket: Object.freeze({
      open: options => call('websocket.open', options),
      send: (connectionId, data) => typeof data === 'string'
        ? call('websocket.send', { connectionId, text: data })
        : call('websocket.send', { connectionId, bytes: bytes(data) }),
      close: connectionId => call('websocket.close', { connectionId }),
    }),
    base64: Object.freeze({ encode: value => call('base64.encode', { value: bytes(value) }), decode: value => new Uint8Array(call('base64.decode', { value })) }),
    text: Object.freeze({ decodeUtf8: value => call('text.decodeUtf8', { value: bytes(value) }) }),
    crypto: Object.freeze({
      randomBytes: size => new Uint8Array(call('crypto.randomBytes', { size })),
      sha256: value => call('crypto.sha256', { value: typeof value === 'string' ? value : bytes(value) }),
      hmacSha256: (key, data) => call('crypto.hmacSha256', { key: typeof key === 'string' ? key : bytes(key), data: typeof data === 'string' ? data : bytes(data) }),
    }),
    time: Object.freeze({ now: () => call('time.now'), sleep: millis => call('time.sleep', { millis }) }),
    storage: Object.freeze({ get: key => call('storage.get', { key }), set: (key, value) => call('storage.set', { key, value }), delete: key => call('storage.delete', { key }) }),
    resource: Object.freeze({ readBytes: path => new Uint8Array(call('resource.readBytes', { path })), readText: path => call('resource.readText', { path }) }),
    cancellation: Object.freeze({ isCancelled: () => call('cancel.isCancelled') }),
    emit: event => call('emit', { event }),
    log: (level, message) => call('log', { level, message: String(message) }),
  });
  // QuickJS 不内置浏览器的 TextDecoder。这里仅补齐 UTF-8 兼容层；新插件应优先使用 host.text.decodeUtf8。
  if (typeof globalThis.TextDecoder !== 'function') {
    globalThis.TextDecoder = class TextDecoder {
      constructor(label = 'utf-8') {
        if (!/^utf-?8$/i.test(String(label))) throw new RangeError('仅支持 UTF-8 TextDecoder');
      }
      decode(value = new Uint8Array()) {
        return globalThis.__sayitHost.text.decodeUtf8(value);
      }
    };
  }
  globalThis.__sayitInitialize = async requestJson => {
    const request = JSON.parse(requestJson);
    if (typeof globalThis.__sayitProvider.initialize === 'function') await globalThis.__sayitProvider.initialize(request);
    return 'null';
  };
  globalThis.__sayitInvoke = async (method, payloadJson) => {
    const fn = globalThis.__sayitProvider[method];
    if (typeof fn !== 'function') {
      if (method === 'onHostEvent') return 'null';
      throw new Error(`插件未实现 ${method}`);
    }
    const value = await fn.call(globalThis.__sayitProvider, JSON.parse(payloadJson));
    return JSON.stringify(value === undefined ? null : value);
  };
  globalThis.__sayitAudio = async audio => {
    const fn = globalThis.__sayitProvider.realtimeAudio;
    if (typeof fn !== 'function') throw new Error('插件未实现 realtimeAudio');
    await fn.call(globalThis.__sayitProvider, audio);
    return 'null';
  };
})();
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(
        source: &str,
        helper: Option<&str>,
    ) -> (PathBuf, PluginRuntimeSpec, ProviderProfile) {
        let root = std::env::temp_dir().join(format!("sayit-js-runtime-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("connector")).unwrap();
        std::fs::write(root.join("connector/index.js"), source).unwrap();
        if let Some(helper) = helper {
            std::fs::write(root.join("connector/helper.js"), helper).unwrap();
        }
        let spec = PluginRuntimeSpec {
            plugin_id: format!("test-{}", uuid::Uuid::new_v4()),
            root: root.clone(),
            entrypoint: root.join("connector/index.js"),
            permissions: vec![],
            allowed_hosts: vec![],
            browser_session: None,
            data_dir: root.join("data"),
            trust: "unsigned".into(),
        };
        let profile = ProviderProfile {
            id: "test-provider".into(),
            kind: "plugin:test".into(),
            display_name: "Test".into(),
            auth_kind: "none".into(),
            capabilities: vec!["asr".into()],
            enabled: true,
            config: json!({}),
            config_fields: vec![],
            actions: vec![],
        };
        (root, spec, profile)
    }

    #[test]
    fn host_whitelist_distinguishes_exact_and_subdomain_rules() {
        let spec = PluginRuntimeSpec {
            plugin_id: "test".into(),
            root: PathBuf::new(),
            entrypoint: PathBuf::new(),
            permissions: vec!["network".into()],
            allowed_hosts: vec!["api.example.com".into(), "*.vendor.test".into()],
            browser_session: None,
            data_dir: PathBuf::new(),
            trust: "unsigned".into(),
        };
        assert!(parse_allowed_url(&spec, "https://api.example.com/v1", &["https"]).is_ok());
        assert!(parse_allowed_url(&spec, "wss://live.vendor.test/ws", &["wss"]).is_ok());
        assert!(parse_allowed_url(&spec, "https://vendor.test", &["https"]).is_err());
        assert!(parse_allowed_url(&spec, "https://evil.example.com", &["https"]).is_err());
        // 未声明 localNetwork：回环明文协议保持拒绝。
        assert!(parse_allowed_url(&spec, "ws://127.0.0.1:8000/ws", &["wss"]).is_err());
        assert!(parse_allowed_url(&spec, "http://localhost:8000", &["https"]).is_err());
    }

    #[test]
    fn local_network_permission_allows_loopback_plaintext_only() {
        let spec = PluginRuntimeSpec {
            plugin_id: "test".into(),
            root: PathBuf::new(),
            entrypoint: PathBuf::new(),
            permissions: vec!["localNetwork".into()],
            allowed_hosts: vec![],
            browser_session: None,
            data_dir: PathBuf::new(),
            trust: "unsigned".into(),
        };
        // 回环地址放行明文与加密协议，且无需出现在 allowedHosts。
        assert!(parse_allowed_url(&spec, "ws://127.0.0.1:8000/v1/ws", &["wss"]).is_ok());
        assert!(parse_allowed_url(&spec, "http://localhost:8000/api", &["https"]).is_ok());
        assert!(parse_allowed_url(&spec, "http://[::1]:9000", &["https"]).is_ok());
        assert!(parse_allowed_url(&spec, "https://127.0.0.1:8443", &["https"]).is_ok());
        // 非回环主机：明文一律拒绝，加密仍受 allowedHosts 约束。
        assert!(parse_allowed_url(&spec, "http://192.168.1.5:8000", &["https"]).is_err());
        assert!(parse_allowed_url(&spec, "http://0.0.0.0:8000", &["https"]).is_err());
        assert!(parse_allowed_url(&spec, "ws://my.localhost.evil:8000", &["wss"]).is_err());
        assert!(parse_allowed_url(&spec, "http://loopback.example.com", &["https"]).is_err());
        assert!(parse_allowed_url(&spec, "https://api.example.com", &["https"]).is_err());
        // 与 https/wss 无关的协议依旧拒绝。
        assert!(parse_allowed_url(&spec, "ftp://127.0.0.1", &["https"]).is_err());
    }

    #[test]
    fn loads_relative_module_and_invokes_readable_javascript() {
        let (root, spec, profile) = fixture(
            "import { suffix } from './helper.js'; export default host => ({ invoke(request) { host.emit({type:'event', text:'ok'}); return {text: request.payload.text + suffix}; } });",
            Some("export const suffix = '-done';"),
        );
        let runtime = JsProviderRuntime::create(
            spec,
            &profile,
            Duration::from_secs(5),
            Arc::new(AtomicBool::new(false)),
            HashMap::new(),
        )
        .unwrap();
        let result = runtime
            .call(
                "invoke",
                &json!({"operation":"test","payload":{"text":"hello"}}),
                Duration::from_secs(5),
            )
            .unwrap();
        assert_eq!(result["text"], "hello-done");
        assert_eq!(runtime.take_events()[0]["text"], "ok");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn provides_utf8_decoder_for_plugin_base64_data() {
        let (root, spec, profile) = fixture(
            "export default host => ({ invoke() { const bytes = host.base64.decode('5rWL6K+V'); return { text: new TextDecoder().decode(bytes), direct: host.text.decodeUtf8(bytes) }; } });",
            None,
        );
        let runtime = JsProviderRuntime::create(
            spec,
            &profile,
            Duration::from_secs(5),
            Arc::new(AtomicBool::new(false)),
            HashMap::new(),
        )
        .unwrap();
        let result = runtime
            .call("invoke", &json!({}), Duration::from_secs(5))
            .unwrap();
        assert_eq!(result["text"], "测试");
        assert_eq!(result["direct"], "测试");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_import_that_escapes_plugin_directory() {
        let (root, spec, profile) = fixture(
            "import '../../outside.js'; export default () => ({});",
            None,
        );
        let result = JsProviderRuntime::create(
            spec,
            &profile,
            Duration::from_secs(2),
            Arc::new(AtomicBool::new(false)),
            HashMap::new(),
        );
        assert!(result.is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn interrupts_javascript_after_deadline() {
        let (root, spec, profile) = fixture(
            "export default () => ({ invoke() { while (true) {} } });",
            None,
        );
        let runtime = JsProviderRuntime::create(
            spec,
            &profile,
            Duration::from_secs(2),
            Arc::new(AtomicBool::new(false)),
            HashMap::new(),
        )
        .unwrap();
        let result = runtime.call("invoke", &json!({}), Duration::from_millis(20));
        assert!(result.is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
