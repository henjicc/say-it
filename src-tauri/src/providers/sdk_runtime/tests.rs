use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::{accept, Message};

use super::{HostRuntimeRecorder, SdkHostBindings};
use crate::providers::credential_store::{CredentialKey, CredentialStore, CredentialStoreHandle};
use crate::providers::plugin::PluginRuntimeSpec;
use crate::providers::plugin_runtime::JsProviderRuntime;
use crate::providers::ProviderProfile;

const SECRET: &str = "sk-sayit-test-secret";

fn fixture(source: &str) -> (PathBuf, PluginRuntimeSpec, ProviderProfile) {
    let root = std::env::temp_dir().join(format!("sayit-sdk-runtime-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(root.join("connector")).unwrap();
    std::fs::write(root.join("connector/index.js"), source).unwrap();
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
        id: "bailian".into(),
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

#[derive(Clone)]
struct TestCredentials;

impl CredentialStore for TestCredentials {
    fn get(&self, key: &CredentialKey) -> Result<Option<String>, String> {
        assert_eq!(key, &CredentialKey::provider("bailian", "apiKey").unwrap());
        Ok(Some(SECRET.into()))
    }

    fn set(&self, _key: &CredentialKey, _value: &str) -> Result<(), String> {
        panic!("test runtime must not write credentials")
    }

    fn delete(&self, _key: &CredentialKey) -> Result<(), String> {
        panic!("test runtime must not delete credentials")
    }
}

#[derive(Clone)]
struct TestRecorder(Arc<Mutex<Vec<Value>>>);

impl HostRuntimeRecorder for TestRecorder {
    fn record(&self, event: Value) {
        self.0.lock().unwrap().push(event);
    }
}

fn bindings(records: Arc<Mutex<Vec<Value>>>) -> SdkHostBindings {
    SdkHostBindings {
        owner_id: "builtin:bailian".into(),
        provider_id: "bailian".into(),
        request_id: "request-1".into(),
        credential_scopes: HashSet::from(["api-key".into()]),
        credential_key: CredentialKey::provider("bailian", "apiKey").unwrap(),
        credentials: CredentialStoreHandle::from_store(Arc::new(TestCredentials)),
        recorder: Arc::new(TestRecorder(records)),
    }
}

fn spawn_chunked_http(chunks: Vec<Vec<u8>>) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0_u8; 4096];
        let _ = stream.read(&mut request);
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
            )
            .unwrap();
        for chunk in chunks {
            write!(stream, "{:X}\r\n", chunk.len()).unwrap();
            stream.write_all(&chunk).unwrap();
            stream.write_all(b"\r\n").unwrap();
            stream.flush().unwrap();
            thread::sleep(Duration::from_millis(20));
        }
        stream.write_all(b"0\r\n\r\n").unwrap();
    });
    (format!("http://{address}/events"), handle)
}

fn spawn_stalled_http() -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0_u8; 4096];
        let _ = stream.read(&mut request);
        thread::sleep(Duration::from_millis(250));
    });
    (format!("http://{address}/stalled"), handle)
}

fn create_sdk_runtime(
    source: &str,
    mut spec: PluginRuntimeSpec,
    profile: &ProviderProfile,
    cancelled: Arc<AtomicBool>,
    inputs: HashMap<String, PathBuf>,
    records: Arc<Mutex<Vec<Value>>>,
) -> JsProviderRuntime {
    spec.permissions.push("localNetwork".into());
    JsProviderRuntime::create_with_sdk_bindings(
        spec,
        profile,
        Duration::from_secs(5),
        cancelled,
        inputs,
        bindings(records),
    )
    .unwrap_or_else(|error| panic!("SDK QuickJS fixture 创建失败 ({source}): {error}"))
}

#[test]
fn streams_sse_and_decodes_utf8_split_across_chunks() {
    let source = r#"
export default () => ({
  async invoke(request) {
    const runtime = globalThis.__sayitCreateRuntimeContext();
    const response = await runtime.transport.fetch(request.payload.url);
    const reader = response.body.getReader();
    const decoder = new TextDecoder('utf-8');
    let text = '';
    while (true) {
      const chunk = await reader.read();
      if (chunk.done) break;
      text += decoder.decode(chunk.value, { stream: true });
    }
    text += decoder.decode();
    reader.releaseLock();
    const event = JSON.parse(text.split('\n\n')[0].slice(6));
    let clearedTimerFired = false;
    const clearedTimer = setTimeout(() => { clearedTimerFired = true; }, 1);
    clearTimeout(clearedTimer);
    await new Promise(resolve => setTimeout(resolve, 5));
    return {
      event,
      text,
      status: response.status,
      contentType: response.headers.get('content-type'),
      globalFetch: typeof globalThis.fetch,
      clearedTimerFired,
    };
  }
});
"#;
    let payload = "data: {\"text\":\"测试\"}\n\n".as_bytes().to_vec();
    let split = "data: {\"text\":\"".as_bytes().len() + 1;
    let chunks = vec![payload[..split].to_vec(), payload[split..].to_vec()];
    let (url, server) = spawn_chunked_http(chunks);
    let (root, spec, profile) = fixture(source);
    let records = Arc::new(Mutex::new(Vec::new()));
    let runtime = create_sdk_runtime(
        source,
        spec,
        &profile,
        Arc::new(AtomicBool::new(false)),
        HashMap::new(),
        records,
    );
    let result = runtime
        .call(
            "invoke",
            &json!({"payload":{"url":url}}),
            Duration::from_secs(3),
        )
        .unwrap();
    assert_eq!(result["event"]["text"], "测试");
    assert_eq!(result["status"], 200);
    assert_eq!(result["contentType"], "text/event-stream");
    assert_eq!(result["globalFetch"], "undefined");
    assert_eq!(result["clearedTimerFired"], false);
    assert_eq!(runtime.sdk_resource_counts(), (0, 0, 0));
    server.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn injects_scoped_credentials_media_logging_and_trace_without_leaks() {
    let source = r#"
export default () => ({
  async invoke() {
    const runtime = globalThis.__sayitCreateRuntimeContext();
    const secret = await runtime.credentials.get('api-key', 'bailian');
    const media = await runtime.media.read('media-1');
    let providerDenied = false;
    let scopeDenied = false;
    let pathDenied = false;
    let networkDenied = false;
    let networkError = '';
    let abortDenied = false;
    try { await runtime.credentials.get('api-key', 'groq'); } catch (_) { providerDenied = true; }
    try { await runtime.credentials.get('other', 'bailian'); } catch (_) { scopeDenied = true; }
    try { await runtime.media.read('/etc/passwd'); } catch (_) { pathDenied = true; }
    const form = new FormData();
    form.append('purpose', 'asr');
    form.append('file', new Blob([media.bytes], { type: media.mimeType }), media.filename);
    try {
      await runtime.transport.fetch('http://127.0.0.1:1/upload', { method: 'POST', body: form });
    } catch (error) {
      networkDenied = true;
      networkError = String(error.message || error);
    }
    const controller = new AbortController();
    controller.abort();
    try { await runtime.transport.fetch('http://127.0.0.1:1/', { signal: controller.signal }); } catch (_) { abortDenied = true; }
    runtime.logger.info(`authorization=${secret}`, { event: 'sdk.request', token: secret, bytes: media.bytes.length });
    const span = runtime.tracer.startSpan('sdk.asr', { modelId: 'paraformer-realtime-v2', authorization: secret });
    span.end(new Error(secret));
    return {
      text: new TextDecoder().decode(media.bytes),
      mimeType: media.mimeType,
      filename: media.filename,
      secretLength: secret.length,
      providerDenied,
      scopeDenied,
      pathDenied,
      networkDenied,
      networkError,
      abortDenied,
      nodeGlobals: [typeof globalThis.process, typeof globalThis.require],
      webGlobals: [typeof globalThis.URL, typeof globalThis.Blob, typeof globalThis.FormData],
      parsedUrl: [new URL('https://example.com/path?q=1').protocol, new URL('https://example.com/path?q=1').pathname],
    };
  }
});
"#;
    let (root, mut spec, profile) = fixture(source);
    let media = root.join("sample.wav");
    std::fs::write(&media, "媒体内容").unwrap();
    // 本测试刻意移除网络权限，证明 SDK adapter 不能绕过 manifest capability。
    spec.permissions.clear();
    let records = Arc::new(Mutex::new(Vec::new()));
    let runtime = JsProviderRuntime::create_with_sdk_bindings(
        spec,
        &profile,
        Duration::from_secs(5),
        Arc::new(AtomicBool::new(false)),
        HashMap::from([("media-1".into(), media)]),
        bindings(records.clone()),
    )
    .unwrap();
    let result = runtime
        .call("invoke", &json!({}), Duration::from_secs(3))
        .unwrap();
    assert_eq!(result["text"], "媒体内容");
    assert_eq!(result["mimeType"], "audio/wav");
    assert_eq!(result["filename"], "sample.wav");
    assert_eq!(result["secretLength"], SECRET.len());
    for key in [
        "providerDenied",
        "scopeDenied",
        "pathDenied",
        "networkDenied",
        "abortDenied",
    ] {
        assert_eq!(result[key], true, "{key} 应被宿主拒绝");
    }
    assert_eq!(result["nodeGlobals"], json!(["undefined", "undefined"]));
    assert_eq!(
        result["webGlobals"],
        json!(["function", "function", "function"])
    );
    assert_eq!(result["parsedUrl"], json!(["https:", "/path"]));
    assert!(result["networkError"]
        .as_str()
        .unwrap()
        .contains("未声明 network"));
    assert_eq!(runtime.sdk_resource_counts(), (0, 0, 0));
    let serialized = serde_json::to_string(&*records.lock().unwrap()).unwrap();
    assert!(!serialized.contains(SECRET));
    assert!(!serialized.contains("authorization"));
    assert!(serialized.contains("sdk.request"));
    assert!(serialized.contains("paraformer-realtime-v2"));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn quickjs_initialize_receives_non_secret_config_only() {
    let source = r#"
export default () => {
  let initializedConfig = null;
  return {
    async initialize(request) { initializedConfig = request.config; },
    async invoke() { return initializedConfig; }
  };
};
"#;
    let (root, spec, mut profile) = fixture(source);
    profile.auth_kind = "api-key".into();
    profile.config = json!({"apiKey": SECRET, "region": "cn-beijing"});
    let runtime = create_sdk_runtime(
        source,
        spec,
        &profile,
        Arc::new(AtomicBool::new(false)),
        HashMap::new(),
        Arc::new(Mutex::new(Vec::new())),
    );

    let result = runtime
        .call("invoke", &json!({}), Duration::from_secs(3))
        .unwrap();

    assert_eq!(result["region"], "cn-beijing");
    assert!(result.get("apiKey").is_none());
    assert!(!serde_json::to_string(&result).unwrap().contains(SECRET));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn propagates_websocket_text_binary_close_and_releases_connection() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut socket = accept(stream).unwrap();
        assert_eq!(socket.read().unwrap().into_text().unwrap(), "hello");
        socket.send(Message::Text("world".into())).unwrap();
        assert_eq!(socket.read().unwrap().into_data(), vec![1, 2, 3]);
        socket.send(Message::Binary(vec![4, 5, 6].into())).unwrap();
        let _ = socket.close(None);
    });
    let source = r#"
export default () => ({
  async invoke(request) {
    const runtime = globalThis.__sayitCreateRuntimeContext();
    const connection = await runtime.realtime.connect(request.payload.url, { headers: { 'x-test': 'yes' } });
    const iterator = connection.messages[Symbol.asyncIterator]();
    await connection.send('hello');
    const text = (await iterator.next()).value.data;
    await connection.send(new Uint8Array([1, 2, 3]));
    const binary = (await iterator.next()).value.data;
    await connection.close(1000, 'done');
    return { text, binary: Array.from(binary) };
  }
});
"#;
    let (root, spec, profile) = fixture(source);
    let records = Arc::new(Mutex::new(Vec::new()));
    let runtime = create_sdk_runtime(
        source,
        spec,
        &profile,
        Arc::new(AtomicBool::new(false)),
        HashMap::new(),
        records,
    );
    let result = runtime
        .call(
            "invoke",
            &json!({"payload":{"url":format!("ws://{address}/realtime")}}),
            Duration::from_secs(3),
        )
        .unwrap();
    assert_eq!(result["text"], "world");
    assert_eq!(result["binary"], json!([4, 5, 6]));
    assert_eq!(runtime.sdk_resource_counts(), (0, 0, 0));
    server.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn stops_blocked_http_on_cancel_and_timeout_without_leaking_streams() {
    let source = r#"
export default () => ({
  async invoke(request) {
    const runtime = globalThis.__sayitCreateRuntimeContext();
    const timer = setTimeout(() => {}, 10_000);
    try {
      await runtime.transport.fetch(request.payload.url);
      return { unexpected: true };
    } finally {
      clearTimeout(timer);
    }
  }
});
"#;

    let (cancel_url, cancel_server) = spawn_stalled_http();
    let (cancel_root, cancel_spec, cancel_profile) = fixture(source);
    let cancelled = Arc::new(AtomicBool::new(false));
    let runtime = create_sdk_runtime(
        source,
        cancel_spec,
        &cancel_profile,
        cancelled.clone(),
        HashMap::new(),
        Arc::new(Mutex::new(Vec::new())),
    );
    let trigger = thread::spawn(move || {
        thread::sleep(Duration::from_millis(40));
        cancelled.store(true, Ordering::Relaxed);
    });
    let error = runtime
        .call(
            "invoke",
            &json!({"payload":{"url":cancel_url}}),
            Duration::from_secs(2),
        )
        .unwrap_err();
    assert!(error.contains("取消") || error.contains("CANCELLED") || error.contains("interrupted"));
    assert_eq!(runtime.sdk_resource_counts(), (0, 0, 0));
    trigger.join().unwrap();
    cancel_server.join().unwrap();
    std::fs::remove_dir_all(cancel_root).unwrap();

    let (timeout_url, timeout_server) = spawn_stalled_http();
    let (timeout_root, timeout_spec, timeout_profile) = fixture(source);
    let runtime = create_sdk_runtime(
        source,
        timeout_spec,
        &timeout_profile,
        Arc::new(AtomicBool::new(false)),
        HashMap::new(),
        Arc::new(Mutex::new(Vec::new())),
    );
    let error = runtime
        .call(
            "invoke",
            &json!({"payload":{"url":timeout_url}}),
            Duration::from_millis(40),
        )
        .unwrap_err();
    assert!(error.contains("超时") || error.contains("TIMEOUT") || error.contains("interrupted"));
    assert_eq!(runtime.sdk_resource_counts(), (0, 0, 0));
    timeout_server.join().unwrap();
    std::fs::remove_dir_all(timeout_root).unwrap();
}
