use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::{accept, Message};

use super::{HostRuntimeRecorder, SdkHostBindings, AI_SDK_BUNDLE_MANIFEST};
use crate::providers::credential_store::{CredentialKey, CredentialStore, CredentialStoreHandle};
use crate::providers::plugin::PluginRuntimeSpec;
use crate::providers::plugin_runtime::JsProviderRuntime;
use crate::providers::ProviderProfile;

const FIXTURE_SECRET: &str = "fixture-sdk-secret";

#[derive(Clone)]
struct BundleCredentials {
    provider_id: String,
}

impl CredentialStore for BundleCredentials {
    fn get(&self, key: &CredentialKey) -> Result<Option<String>, String> {
        assert_eq!(
            key,
            &CredentialKey::provider(&self.provider_id, "apiKey").unwrap()
        );
        Ok(Some(FIXTURE_SECRET.into()))
    }

    fn set(&self, _key: &CredentialKey, _value: &str) -> Result<(), String> {
        panic!("SDK bundle fixture must not write credentials")
    }

    fn delete(&self, _key: &CredentialKey) -> Result<(), String> {
        panic!("SDK bundle fixture must not delete credentials")
    }
}

#[derive(Clone)]
struct BundleRecorder(Arc<Mutex<Vec<Value>>>);

impl HostRuntimeRecorder for BundleRecorder {
    fn record(&self, event: Value) {
        self.0.lock().unwrap().push(event);
    }
}

fn fixture(
    source: &str,
    provider_id: &str,
    scopes: &[&str],
) -> (PathBuf, PluginRuntimeSpec, ProviderProfile, SdkHostBindings) {
    let root = std::env::temp_dir().join(format!("sayit-sdk-bundle-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(root.join("connector")).unwrap();
    std::fs::write(root.join("connector/index.js"), source).unwrap();
    let spec = PluginRuntimeSpec {
        plugin_id: format!("sdk-{provider_id}-{}", uuid::Uuid::new_v4()),
        source_namespace: "@henjicc/ai-sdk".into(),
        capabilities: vec![],
        secret_fields: vec![],
        credentials: None,
        root: root.clone(),
        entrypoint: root.join("connector/index.js"),
        permissions: vec!["localNetwork".into()],
        allowed_hosts: vec![],
        browser_session: None,
        data_dir: root.join("data"),
        trust: "unsigned".into(),
    };
    let profile = ProviderProfile {
        id: provider_id.into(),
        kind: format!("sdk:{provider_id}"),
        display_name: format!("SDK {provider_id}"),
        auth_kind: "system-credential".into(),
        capabilities: vec![],
        enabled: true,
        config: json!({}),
        config_fields: vec![],
        actions: vec![],
    };
    let scope_set: HashSet<String> = scopes.iter().map(|value| (*value).to_string()).collect();
    let bindings = SdkHostBindings {
        owner_id: format!("builtin:{provider_id}"),
        provider_id: provider_id.into(),
        request_id: "sdk-bundle-fixture".into(),
        credential_scopes: scope_set.clone(),
        credential_key: CredentialKey::provider(provider_id, "apiKey").unwrap(),
        credentials: CredentialStoreHandle::from_store(Arc::new(BundleCredentials {
            provider_id: provider_id.into(),
        })),
        recorder: Arc::new(BundleRecorder(Arc::new(Mutex::new(Vec::new())))),
    };
    (root, spec, profile, bindings)
}

fn create_runtime(
    source: &str,
    provider_id: &str,
    scopes: &[&str],
) -> (PathBuf, JsProviderRuntime) {
    let (root, spec, profile, bindings) = fixture(source, provider_id, scopes);
    let runtime = JsProviderRuntime::create_with_sdk_bindings(
        spec,
        &profile,
        Duration::from_secs(5),
        Arc::new(AtomicBool::new(false)),
        HashMap::new(),
        bindings,
    )
    .unwrap();
    (root, runtime)
}

fn create_runtime_with_cancelled(
    source: &str,
    provider_id: &str,
    scopes: &[&str],
    cancelled: Arc<AtomicBool>,
) -> (PathBuf, JsProviderRuntime) {
    let (root, spec, profile, bindings) = fixture(source, provider_id, scopes);
    let runtime = JsProviderRuntime::create_with_sdk_bindings(
        spec,
        &profile,
        Duration::from_secs(5),
        cancelled,
        HashMap::new(),
        bindings,
    )
    .unwrap();
    (root, runtime)
}

fn spawn_http_response(content_type: &str, body: Vec<u8>) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let content_type = content_type.to_string();
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0_u8; 16 * 1024];
        let _ = stream.read(&mut request);
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .unwrap();
        stream.write_all(&body).unwrap();
        stream.flush().unwrap();
    });
    (format!("http://{address}"), handle)
}

#[test]
fn loads_exact_sdk_bundle_and_discovers_only_requested_capabilities() {
    let manifest: Value = serde_json::from_str(AI_SDK_BUNDLE_MANIFEST).unwrap();
    assert_eq!(manifest["sdk"]["version"], "0.2.2");
    assert_eq!(
        manifest["sdk"]["integrity"],
        "sha512-6VMyZwxz/oVTKmGAlfMQyAjVxLgjJISDdE5df2sQhvFpxAp4cXJvB3kvMWKIjI9HIl+Y7CsOqfpA9lvz2MM6QA=="
    );
    assert!(manifest["bundles"]["capabilities"]["bytes"]
        .as_u64()
        .is_some_and(|bytes| bytes > 0));
    assert!(manifest["bundles"]["groq"]["bytes"]
        .as_u64()
        .is_some_and(|bytes| bytes > 0));
    assert_eq!(manifest["bundles"]["groq"]["modules"], 99);
    assert!(manifest["bundles"]["llmModules"]["bytes"]
        .as_u64()
        .is_some_and(|bytes| bytes > 0 && bytes < 32 * 1024));
    assert_eq!(manifest["bundles"]["llmModules"]["modules"], 12);

    let source = r#"
export default () => ({
  async invoke() {
    const sdk = globalThis.__sayitCreateSdkRuntime({
      sources: [
        'bailian-speech-recognition',
        'bailian-speech-recognition-realtime',
        'bailian-translation',
      ],
    });
    try {
      const descriptors = sdk.capabilities.descriptors();
      return {
        version: sdk.version,
        namespace: sdk.sourceNamespace,
        count: descriptors.length,
        ids: descriptors.map(value => value.id),
        sources: descriptors.map(value => value.source),
        globals: [typeof globalThis.process, typeof globalThis.require, typeof globalThis.fetch],
      };
    } finally {
      await sdk.dispose();
      await globalThis.__sayitDisposeAllSdkRuntimes();
      globalThis.__sayitDisposeRuntimeContext();
    }
  }
});
"#;
    let (root, runtime) = create_runtime(source, "bailian", &["speech-recognition", "translation"]);
    let result = runtime
        .call("invoke", &json!({}), Duration::from_secs(3))
        .unwrap();
    assert_eq!(result["version"], "0.2.2");
    assert_eq!(result["namespace"], "@henjicc/ai-sdk");
    assert_eq!(result["count"], 12);
    assert_eq!(
        result["globals"],
        json!(["undefined", "undefined", "undefined"])
    );
    assert!(result["ids"]
        .as_array()
        .unwrap()
        .iter()
        .all(|id| id.as_str().unwrap().starts_with("bailian.")));
    assert!(result["sources"]
        .as_array()
        .unwrap()
        .iter()
        .all(|source| { source == &json!({"kind":"builtin","namespace":"@henjicc/ai-sdk"}) }));
    assert_eq!(runtime.sdk_resource_counts(), (0, 0, 0));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn executes_non_realtime_asr_and_translation_through_host_transport() {
    let (asr_base, asr_server) = spawn_http_response(
        "application/json",
        r#"{"choices":[{"message":{"content":"脚本化识别"}}]}"#
            .as_bytes()
            .to_vec(),
    );
    let (translation_base, translation_server) = spawn_http_response(
        "application/json",
        br#"{"id":"translation-1","model":"qwen-mt-plus","choices":[{"message":{"content":"scripted translation"},"finish_reason":"stop"}],"usage":{"prompt_tokens":3,"completion_tokens":2,"total_tokens":5}}"#.to_vec(),
    );
    let source = r#"
export default () => ({
  async invoke(request) {
    const sdk = globalThis.__sayitCreateSdkRuntime({
      sources: ['bailian-speech-recognition', 'bailian-translation'],
      capabilityOptions: {
        bailianSpeechRecognition: { compatibleBaseUrl: `${request.payload.asrBase}/compatible-mode/v1` },
        bailianTranslation: { endpoint: `${request.payload.translationBase}/translate`, defaultStream: false },
      },
    });
    const events = [];
    try {
      const asr = await sdk.capabilities.execute(
        'bailian.speech-recognition.qwen3-asr-flash',
        { audio: { kind: 'bytes', bytes: new Uint8Array([1, 2, 3]), mediaType: 'audio/wav' } },
        { requestId: 'scripted-asr', onEvent: event => events.push(event.type) },
      );
      const translation = await sdk.capabilities.execute(
        'bailian.translation.qwen-mt-plus',
        { source: '脚本化翻译', sourceLanguage: 'zh', targetLanguage: 'en' },
        { requestId: 'scripted-translation', onEvent: event => events.push(event.type) },
      );
      return { asr: asr.text, translation: translation.translations[0].text, usage: translation.usage, events };
    } finally {
      await sdk.dispose();
      globalThis.__sayitDisposeRuntimeContext();
    }
  }
});
"#;
    let (root, runtime) = create_runtime(source, "bailian", &["speech-recognition", "translation"]);
    let result = runtime
        .call(
            "invoke",
            &json!({"payload":{"asrBase":asr_base,"translationBase":translation_base}}),
            Duration::from_secs(3),
        )
        .unwrap();
    assert_eq!(result["asr"], "脚本化识别");
    assert_eq!(result["translation"], "scripted translation");
    assert_eq!(
        result["usage"],
        json!({"inputTokens":3,"outputTokens":2,"totalTokens":5})
    );
    assert!(result["events"].as_array().unwrap().len() >= 5);
    assert_eq!(runtime.sdk_resource_counts(), (0, 0, 0));
    asr_server.join().unwrap();
    translation_server.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn cancelling_translation_aborts_http_stream_and_releases_resources() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (request_tx, request_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0_u8; 16 * 1024];
        let _ = stream.read(&mut request);
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 4096\r\nConnection: close\r\n\r\n{{"
        )
        .unwrap();
        stream.flush().unwrap();
        request_tx.send(()).unwrap();
        let _ = release_rx.recv_timeout(Duration::from_secs(2));
    });
    let source = r#"
export default () => ({
  async invoke(request) {
    const sdk = globalThis.__sayitCreateSdkRuntime({
      sources: ['bailian-translation'],
      capabilityOptions: {
        bailianTranslation: { endpoint: `${request.payload.baseUrl}/translate`, defaultStream: false },
      },
    });
    try {
      return await sdk.capabilities.execute(
        'bailian.translation.qwen-mt-flash',
        { source: '取消', sourceLanguage: 'zh', targetLanguage: 'en' },
        { requestId: 'cancelled-translation' },
      );
    } finally {
      await sdk.dispose();
      globalThis.__sayitDisposeRuntimeContext();
    }
  }
});
"#;
    let cancelled = Arc::new(AtomicBool::new(false));
    let (root, runtime) =
        create_runtime_with_cancelled(source, "bailian", &["translation"], cancelled.clone());
    let cancel_worker = thread::spawn(move || {
        request_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        cancelled.store(true, std::sync::atomic::Ordering::Release);
    });
    let result = runtime.call(
        "invoke",
        &json!({"payload":{"baseUrl":format!("http://{address}")}}),
        Duration::from_secs(5),
    );
    cancel_worker.join().unwrap();
    assert!(result.unwrap_err().contains("取消"));
    assert_eq!(runtime.sdk_resource_counts(), (0, 0, 0));
    let _ = release_tx.send(());
    server.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn executes_groq_stream_through_host_transport_without_loading_capabilities() {
    let sse = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"scripted groq\"},\"finish_reason\":\"stop\"}],",
        "\"usage\":{\"prompt_tokens\":2,\"completion_tokens\":2,\"total_tokens\":4}}\n\n",
        "data: [DONE]\n\n"
    );
    let (groq_base, groq_server) =
        spawn_http_response("text/event-stream", sse.as_bytes().to_vec());
    let source = r#"
export default () => ({
  async invoke(request) {
    const sdk = globalThis.__sayitCreateSdkRuntime({ sources: ['groq-llm'] });
    const events = [];
    try {
      let mediaRejected = '';
      try {
        await sdk.groq.run(
          {
            baseUrl: request.payload.baseUrl,
            messages: [{ role: 'user', content: [{ type: 'image', imageUrl: 'https://example.invalid/x.png' }] }],
          },
          'scripted-groq-media',
        );
      } catch (error) {
        mediaRejected = String(error);
      }
      const output = await sdk.groq.run(
        { baseUrl: request.payload.baseUrl, messages: [{ role: 'user', content: 'fixture' }] },
        'scripted-groq',
        { onEvent: event => events.push(event) },
      );
      return {
        output: output.output,
        usage: output.usage,
        events,
        capabilitiesLoaded: sdk.capabilities !== undefined,
        providerId: sdk.groq.providerId,
        modelId: sdk.groq.defaultModelId,
        mediaRejected,
      };
    } finally {
      await sdk.dispose();
      globalThis.__sayitDisposeRuntimeContext();
    }
  }
});
"#;
    let (root, runtime) = create_runtime(source, "groq", &["llm"]);
    let result = runtime
        .call(
            "invoke",
            &json!({"payload":{"baseUrl":groq_base}}),
            Duration::from_secs(3),
        )
        .unwrap();
    assert_eq!(result["output"], "scripted groq");
    assert_eq!(result["usage"]["totalTokens"], 4);
    assert_eq!(
        result["events"][0],
        json!({"type":"Token","data":"scripted groq"})
    );
    assert_eq!(result["capabilitiesLoaded"], false);
    assert_eq!(result["providerId"], "groq");
    assert_eq!(result["modelId"], "openai/gpt-oss-20b");
    assert!(
        result["mediaRejected"]
            .as_str()
            .is_some_and(|error| error.contains("当前只接受文本消息")),
        "{result}"
    );
    assert_eq!(runtime.sdk_resource_counts(), (0, 0, 0));
    groq_server.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn executes_realtime_asr_protocol_through_host_websocket_bridge() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut socket = accept(stream).unwrap();
        socket
            .send(Message::Text(
                r#"{"type":"session.created","session":{"id":"scripted-session"}}"#.into(),
            ))
            .unwrap();
        let update = socket.read().unwrap().into_text().unwrap();
        assert!(update.contains("session.update"));
        socket
            .send(Message::Text(
                r#"{"type":"session.updated","session":{"id":"scripted-session"}}"#.into(),
            ))
            .unwrap();
        let audio = socket.read().unwrap().into_text().unwrap();
        assert!(audio.contains("input_audio_buffer.append"));
        let finish = socket.read().unwrap().into_text().unwrap();
        assert!(finish.contains("session.finish"));
        socket
            .send(Message::Text(
                r#"{"type":"conversation.item.input_audio_transcription.completed","transcript":"脚本化实时识别"}"#.into(),
            ))
            .unwrap();
        socket
            .send(Message::Text(r#"{"type":"session.finished"}"#.into()))
            .unwrap();
        let _ = socket.close(None);
    });
    let source = r#"
export default () => ({
  async invoke(request) {
    const hostRuntime = globalThis.__sayitCreateRuntimeContext();
    const hostConnect = hostRuntime.realtime.connect;
    hostRuntime.realtime.connect = (_url, options) => hostConnect(request.payload.url, options);
    const capabilities = globalThis.__sayitAiSdkCapabilities.createSayItCapabilityRuntime(hostRuntime, {
      sources: ['bailian-speech-recognition-realtime'],
      bailianSpeechRecognitionRealtime: { qwenWebSocketUrl: 'wss://scripted.invalid/realtime' },
    });
    const events = [];
    try {
      const session = await capabilities.openSession(
        'bailian.speech-recognition.qwen3-asr-flash-realtime',
        { mediaType: 'audio/pcm', sampleRateHz: 16000, channels: 1 },
        { requestId: 'scripted-realtime', onEvent: event => events.push(event.type) },
      );
      await session.send({ bytes: new Uint8Array([1, 2, 3]) });
      const output = await session.finish();
      await session.close();
      return { text: output.text, events };
    } finally {
      await capabilities.dispose();
      globalThis.__sayitDisposeRuntimeContext();
    }
  }
});
"#;
    let (root, runtime) = create_runtime(source, "bailian", &["speech-recognition"]);
    let result = runtime
        .call(
            "invoke",
            &json!({"payload":{"url":format!("ws://{address}/realtime")}}),
            Duration::from_secs(3),
        )
        .unwrap();
    assert_eq!(result["text"], "脚本化实时识别");
    assert_eq!(result["events"], json!(["started", "final", "completed"]));
    assert_eq!(runtime.sdk_resource_counts(), (0, 0, 0));
    server.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}
