use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use crate::cancellation::CancellationFlag;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::{accept, Message};

use super::online::{BuiltinSdkRuntime, BuiltinSdkScope};
use super::{HostRuntimeRecorder, SdkHostBindings, AI_SDK_BUNDLE_MANIFEST};
use crate::providers::credential_store::{CredentialKey, CredentialStore, CredentialStoreHandle};
use crate::providers::plugin::PluginRuntimeSpec;
use crate::providers::plugin_runtime::JsProviderRuntime;
use crate::providers::ProviderProfile;

const FIXTURE_SECRET: &str = "fixture-sdk-secret";

#[derive(Clone)]
struct BundleCredentials {
    provider_id: String,
    value: Option<&'static str>,
}

impl CredentialStore for BundleCredentials {
    fn get(&self, key: &CredentialKey) -> Result<Option<String>, String> {
        assert_eq!(
            key,
            &CredentialKey::provider(&self.provider_id, "apiKey").unwrap()
        );
        Ok(self.value.map(str::to_owned))
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
        keep_loaded: false,
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
            value: Some(FIXTURE_SECRET),
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
        Arc::new(CancellationFlag::new(false)),
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
    cancelled: Arc<CancellationFlag>,
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
    assert_eq!(manifest["sdk"]["version"], "0.18.0");
    assert_eq!(
        manifest["sdk"]["integrity"],
        "sha512-qmX9AInyeTuxjv/qABjhYiMpPeDEoLMl3a+cT0oxJYm4b4mKkJhUY7fpkdCRTjNQrlqE9iCeo3fRE1y1QYIC8Q=="
    );
    assert!(manifest["bundles"]["capabilities"]["bytes"]
        .as_u64()
        .is_some_and(|bytes| bytes > 0));
    assert!(manifest["bundles"]["llm"]["bytes"]
        .as_u64()
        .is_some_and(|bytes| bytes > 0 && bytes < 128 * 1024));
    assert!(manifest["bundles"]["llmModules"]["bytes"]
        .as_u64()
        .is_some_and(|bytes| bytes > 0 && bytes < 32 * 1024));
    // 0.11.0 起描述校验会检查价格，多引入 pricing/capabilityPricing.js。
    assert_eq!(manifest["bundles"]["llmModules"]["modules"], 13);
    let pricing = &manifest["pricing"];
    assert_eq!(
        pricing["capabilities"]["bailian.speech-recognition.fun-asr-realtime-2026-02-28"]["billing"],
        "audio-duration"
    );
    assert_eq!(
        pricing["capabilities"]["bailian.translation.qwen-mt-flash"]["billing"],
        "tokens"
    );
    assert_eq!(
        pricing["capabilities"]["volcengine-speech.translation.doubao-mt"]["billing"],
        "tokens"
    );
    assert_eq!(pricing["llm"]["bailian"]["qwen3.8-flash"]["currency"], "CNY");

    let source = r#"
export default () => ({
  async invoke() {
    const sdk = globalThis.__sayitCreateSdkRuntime({
      sources: [
        'bailian-speech-recognition',
        'bailian-speech-recognition-realtime',
        'bailian-translation',
        'volcengine-speech-recognition',
        'volcengine-speech-recognition-realtime',
        'volcengine-translation',
        'siliconflow-speech-recognition',
        'groq-speech-recognition',
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
    assert_eq!(result["version"], "0.18.0");
    assert_eq!(result["namespace"], "@henjicc/ai-sdk");
    assert_eq!(result["count"], 25);
    assert_eq!(
        result["globals"],
        json!(["undefined", "undefined", "undefined"])
    );
    let ids = result["ids"].as_array().unwrap();
    for model in crate::providers::registry::models().iter().filter(|model| model.sdk_capability_source.is_some()) {
        assert!(ids.iter().any(|id| id.as_str() == model.capability_id.as_deref()), "宿主目录中的模型 {} 没有 SDK module", model.id);
    }
    for expected in [
        "bailian.speech-recognition.qwen-audio-3.1-asr-flash",
        "bailian.speech-recognition.qwen-audio-3.1-asr-flash-filetrans",
        "bailian.speech-recognition.qwen-audio-3.1-asr-flash-streaming",
        "volcengine.speech-recognition.seedasr-2.0-file",
        "volcengine.speech-recognition.bigasr-file-turbo",
        "volcengine.speech-recognition.seedasr-2.0-realtime",
        "siliconflow.speech-recognition.FunAudioLLM/SenseVoiceSmall",
        "bailian.speech-recognition.qwen-audio-3.0-asr-flash",
        "bailian.speech-recognition.qwen-audio-3.0-asr-flash-filetrans",
        "bailian.speech-recognition.qwen-audio-3.0-asr-flash-streaming",
        "groq.speech-recognition.whisper-large-v3-turbo",
        "groq.speech-recognition.whisper-large-v3",
    ] {
        assert!(
            ids.iter().any(|id| id.as_str() == Some(expected)),
            "缺少 {expected}"
        );
    }
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
fn builtin_doubao_models_execute_against_the_speech_service_contract() {
    let source = concat!(
        r#"
const createContext = globalThis.__sayitCreateRuntimeContext;
globalThis.__sayitCreateRuntimeContext = () => {
  const runtime = createContext();
  const bytes = new Uint8Array([1, 2, 3]);
  runtime.media.describe = async () => ({ size: bytes.length, mimeType: 'audio/wav', filename: 'fixture.wav' });
  runtime.media.readChunk = async (_ref, offset, length) => bytes.slice(offset, offset + length);
  runtime.transport.fetchStream = async (url, init) => {
    if (url !== 'https://openspeech.bytedance.com/api/v3/auc/bigmodel/recognize/flash' ||
        init.headers.get('X-Api-Resource-Id') !== 'volc.bigasr.auc_turbo' ||
        init.headers.get('X-Api-Key') !== 'fixture-sdk-secret') throw new Error('wrong speech endpoint or credentials');
    let body = '';
    for await (const chunk of init.body) body += new TextDecoder().decode(chunk);
    const input = JSON.parse(body);
    if (input.audio.data !== 'AQID' || input.audio.language !== 'ja-JP' || !input.request.show_utterances) {
      throw new Error('wrong flash transcription input');
    }
    return new Response(JSON.stringify({
      audio_info: { duration: 300 },
      result: { text: 'fixture', utterances: [{ text: 'fixture', start_time: 10, end_time: 290,
        words: [{ text: 'fixture', start_time: 10, end_time: 290 }] }] },
    }), { headers: { 'X-Api-Status-Code': '20000000' } });
  };
  runtime.transport.fetch = async (url, init) => {
    if (url !== 'https://openspeech.bytedance.com/api/v3/machine_translation/matx_translate' ||
        new Headers(init.headers).get('X-Api-Resource-Id') !== 'volc.speech.mt' ||
        new Headers(init.headers).get('X-Api-Key') !== 'fixture-sdk-secret') throw new Error('wrong translation endpoint or credentials');
    const input = JSON.parse(init.body);
    if (input.source_language !== undefined || input.target_language !== 'zh-Hant' || input.text_list[0] !== 'fixture') {
      throw new Error('wrong machine translation input');
    }
    return new Response(JSON.stringify({ code: 20000000, data: { translation_list: [{ translation: '測試' }] } }));
  };
  return runtime;
};
"#,
        include_str!("../../../../sdk-runtime/builtin-online-entry.js")
    );
    let (root, runtime) = create_runtime(source, "volcengine-speech", &["speech-recognition", "translation"]);
    let asr = runtime.call("invoke", &json!({
        "operation": "capability.execute", "source": "volcengine-speech-recognition",
        "moduleId": "volcengine.speech-recognition.bigasr-file-turbo", "requestId": "fixture-file",
        "input": {"audio": {"kind": "media-ref", "ref": "fixture"}, "language": "ja", "timestamps": true}
    }), Duration::from_secs(3)).unwrap();
    assert_eq!(asr["segments"][0]["words"][0]["endMs"], 290);
    let translation = runtime.call("invoke", &json!({
        "operation": "capability.execute", "source": "volcengine-translation",
        "moduleId": "volcengine-speech.translation.doubao-mt", "requestId": "fixture-translation",
        "input": {"source": "fixture", "sourceLanguage": "auto", "targetLanguage": "zh_tw"}
    }), Duration::from_secs(3)).unwrap();
    assert_eq!(translation["translations"][0]["text"], "測試");
    assert_eq!(runtime.sdk_resource_counts(), (0, 0, 0));
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn volcengine_mt_reads_the_saved_speech_key_before_network() {
    let credentials = CredentialStoreHandle::from_store(Arc::new(BundleCredentials {
        provider_id: crate::providers::VOLCENGINE_PROVIDER_ID.into(),
        value: None,
    }));
    let result = super::online::translate_sdk(
        crate::providers::volcengine_profile(),
        credentials,
        "volcengine-doubao-mt".into(),
        "fixture".into(),
        "auto".into(),
        "zh_tw".into(),
        tokio_util::sync::CancellationToken::new(),
        |_| {},
    )
    .await;
    assert!(result.unwrap_err().contains("API key is not configured"));
}

#[tokio::test]
async fn volcengine_mt_rejects_an_unsupported_language_before_network() {
    let credentials = CredentialStoreHandle::from_store(Arc::new(BundleCredentials {
        provider_id: crate::providers::VOLCENGINE_PROVIDER_ID.into(),
        value: None,
    }));
    let result = super::online::translate_sdk(
        crate::providers::volcengine_profile(),
        credentials,
        "volcengine-doubao-mt".into(),
        "fixture".into(),
        "auto".into(),
        "yue".into(),
        tokio_util::sync::CancellationToken::new(),
        |_| {},
    )
    .await;
    assert!(result.unwrap_err().contains("当前翻译模型不支持目标语言"));
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
    let cancelled = Arc::new(CancellationFlag::new(false));
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
fn executes_chat_and_responses_streams_through_host_transport_without_loading_capabilities() {
    let chat_sse = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"scripted chat\"},\"finish_reason\":\"stop\"}],",
        "\"usage\":{\"prompt_tokens\":2,\"completion_tokens\":2,\"total_tokens\":4}}\n\n",
        "data: [DONE]\n\n"
    );
    let responses_sse = concat!(
        "data: {\"type\":\"response.reasoning_text.delta\",\"delta\":\"想\"}\n\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"你好\"}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":3,\"output_tokens\":2}}}\n\n"
    );
    let (chat_base, chat_server) =
        spawn_http_response("text/event-stream", chat_sse.as_bytes().to_vec());
    let (responses_base, responses_server) =
        spawn_http_response("text/event-stream", responses_sse.as_bytes().to_vec());
    let source = r#"
export default () => ({
  async invoke(request) {
    const sdk = globalThis.__sayitCreateSdkRuntime({ sources: ['llm'] });
    const events = [];
    const base = { providerId: 'deepseek', modelId: 'deepseek-flash' };
    try {
      let mediaRejected = '';
      try {
        await sdk.llm.run(
          {
            ...base,
            baseUrl: request.payload.chatBase,
            messages: [{ role: 'user', content: [{ type: 'image', imageUrl: 'https://example.invalid/x.png' }] }],
          },
          'scripted-media',
        );
      } catch (error) {
        mediaRejected = String(error);
      }
      const chat = await sdk.llm.run(
        { ...base, baseUrl: request.payload.chatBase, messages: [{ role: 'user', content: 'fixture' }] },
        'scripted-chat',
        { onEvent: event => events.push(event) },
      );
      const responses = await sdk.llm.run(
        {
          ...base,
          baseUrl: request.payload.responsesBase,
          apiProtocol: 'openai-responses',
          messages: [{ role: 'user', content: 'fixture' }],
        },
        'scripted-responses',
      );
      return {
        chat: chat.output,
        chatUsage: chat.usage,
        events,
        responses: responses.output,
        responsesReasoning: responses.reasoningOutput,
        responsesUsage: responses.usage,
        capabilitiesLoaded: sdk.capabilities !== undefined,
        mediaRejected,
      };
    } finally {
      await sdk.dispose();
      globalThis.__sayitDisposeRuntimeContext();
    }
  }
});
"#;
    let (root, runtime) = create_runtime(source, "deepseek", &["llm"]);
    let result = runtime
        .call(
            "invoke",
            &json!({"payload":{"chatBase":chat_base,"responsesBase":responses_base}}),
            Duration::from_secs(3),
        )
        .unwrap();
    assert_eq!(result["chat"], "scripted chat");
    assert_eq!(result["chatUsage"]["totalTokens"], 4);
    assert_eq!(
        result["events"][0],
        json!({"type":"Token","data":"scripted chat"})
    );
    assert_eq!(result["responses"], "你好");
    assert_eq!(result["responsesReasoning"], "想");
    assert_eq!(result["responsesUsage"]["outputTokens"], 2);
    assert_eq!(result["capabilitiesLoaded"], false);
    assert!(
        result["mediaRejected"]
            .as_str()
            .is_some_and(|error| error.contains("当前只接受文本消息")),
        "{result}"
    );
    assert_eq!(runtime.sdk_resource_counts(), (0, 0, 0));
    chat_server.join().unwrap();
    responses_server.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn volcengine_realtime_looks_up_the_saved_speech_key() {
    let profile = crate::providers::volcengine_profile();
    let credentials = CredentialStoreHandle::from_store(Arc::new(BundleCredentials {
        provider_id: profile.id.clone(),
        value: None,
    }));
    let scope = BuiltinSdkScope::speech_recognition(&profile).unwrap();
    let route = crate::providers::registry::builtin_sdk_asr_route("seedasr-2.0-realtime").unwrap();
    let request_id = "fixture-volcengine-realtime";
    let runtime = BuiltinSdkRuntime::create(
        &profile,
        credentials,
        scope,
        request_id,
        Arc::new(CancellationFlag::new(false)),
        HashMap::new(),
    )
    .unwrap();
    let error = runtime
        .realtime_start(
            &route.source,
            &route.module_id,
            json!({"mediaType": "audio/pcm", "sampleRateHz": 16000, "channels": 1}),
            request_id,
        )
        .unwrap_err();
    assert!(error.contains("API key is not configured"), "{error}");
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

#[test]
fn fun_asr_bundle_ignores_empty_intermediate_frames_and_keeps_final() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut socket = accept(stream).unwrap();
        let run_task = socket.read().unwrap().into_text().unwrap();
        assert!(run_task.contains("run-task"));
        socket
            .send(Message::Text(
                r#"{"header":{"event":"task-started","task_id":"scripted-fun"}}"#.into(),
            ))
            .unwrap();
        socket
            .send(Message::Text(
                r#"{"header":{"event":"result-generated","task_id":"scripted-fun"},"payload":{"output":{"sentence":{"sentence_id":1,"sentence_begin":true,"sentence_end":false,"begin_time":0,"end_time":null,"text":"","words":[]}},"usage":null}}"#.into(),
            ))
            .unwrap();
        socket
            .send(Message::Text(
                r#"{"header":{"event":"result-generated","task_id":"scripted-fun"},"payload":{"output":{"sentence":{"sentence_id":1,"sentence_end":false,"begin_time":0,"end_time":null,"text":"","words":[]}},"usage":null}}"#.into(),
            ))
            .unwrap();
        assert!(socket.read().unwrap().is_binary());
        let finish_task = socket.read().unwrap().into_text().unwrap();
        assert!(finish_task.contains("finish-task"));
        socket
            .send(Message::Text(
                r#"{"header":{"event":"result-generated","task_id":"scripted-fun"},"payload":{"output":{"sentence":{"sentence_id":1,"sentence_end":true,"begin_time":0,"end_time":820,"text":"脚本化实时识别","words":[]}},"usage":{"duration":1}}}"#.into(),
            ))
            .unwrap();
        socket
            .send(Message::Text(
                r#"{"header":{"event":"task-finished","task_id":"scripted-fun"},"payload":{}}"#
                    .into(),
            ))
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
      bailianSpeechRecognitionRealtime: { funWebSocketUrl: 'wss://scripted.invalid/realtime' },
    });
    const events = [];
    try {
      const session = await capabilities.openSession(
        'bailian.speech-recognition.fun-asr-realtime',
        { mediaType: 'audio/pcm', sampleRateHz: 16000, channels: 1 },
        { requestId: 'scripted-fun-realtime', onEvent: event => events.push(event.type) },
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
