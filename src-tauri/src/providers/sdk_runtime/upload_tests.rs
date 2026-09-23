use super::*;

// 服务端同样只保留头部与尾部，避免用测试本身的整文件缓冲掩盖宿主问题。
fn drain_request(stream: &mut std::net::TcpStream) -> (String, Vec<u8>, Vec<u8>, usize) {
    stream
        .set_read_timeout(Some(Duration::from_secs(30)))
        .unwrap();
    let mut headers = Vec::new();
    while !headers.ends_with(b"\r\n\r\n") {
        let mut byte = [0];
        stream.read_exact(&mut byte).unwrap();
        headers.push(byte[0]);
        assert!(headers.len() < 16 * 1024);
    }
    let headers = String::from_utf8(headers).unwrap();
    let length = headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().unwrap())
        })
        .unwrap_or(0);
    let mut prefix = Vec::new();
    let mut tail = Vec::new();
    let mut total = 0;
    let mut buffer = [0; 64 * 1024];
    while total < length {
        let count = stream.read(&mut buffer).unwrap();
        assert!(count > 0, "上传提前结束：{total}/{length}");
        if prefix.len() < 4096 {
            prefix.extend_from_slice(&buffer[..count.min(4096 - prefix.len())]);
        }
        tail.extend_from_slice(&buffer[..count]);
        if tail.len() > 512 {
            tail.drain(..tail.len() - 512);
        }
        total += count;
    }
    assert_eq!(total, length);
    (headers, prefix, tail, total)
}

fn reply(stream: &mut std::net::TcpStream, body: &str) {
    write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
}

#[test]
fn native_upload_streams_more_than_the_js_heap_and_keeps_file_last() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let request = drain_request(&mut stream);
        reply(&mut stream, "{}");
        request
    });
    let source = r#"
export default () => ({ async invoke(request) {
  const runtime = globalThis.__sayitCreateRuntimeContext();
  runtime.media.read = runtime.media.describe = runtime.media.readChunk = () => { throw Error('native upload must not read through JS'); };
  try {
    const response = await runtime.transport.uploadFile(request.payload.url, {
      ref: 'audio', fields: [['key', 'fixture-object'], ['policy', 'fixture-policy']],
      fileField: 'file', maxBytes: 1000000000, signal: new AbortController().signal,
    });
    return { status: response.status, text: await response.text() };
  } finally { globalThis.__sayitDisposeRuntimeContext(); }
} });
"#;
    let (root, spec, profile) = fixture(source);
    let file = root.join("long.wav");
    let size = 80 * 1024 * 1024;
    std::fs::File::create(&file).unwrap().set_len(size).unwrap();
    let runtime = create_sdk_runtime(
        source,
        spec,
        &profile,
        Arc::new(AtomicBool::new(false)),
        HashMap::from([("audio".into(), file)]),
        Arc::new(Mutex::new(Vec::new())),
    );
    let result = runtime
        .call(
            "invoke",
            &json!({"payload":{"url":url}}),
            Duration::from_secs(30),
        )
        .unwrap();
    assert_eq!(result["status"], 200);
    assert_eq!(runtime.sdk_resource_counts(), (0, 0, 0));
    let (headers, prefix, tail, count) = server.join().unwrap();
    assert!(headers.to_lowercase().contains("multipart/form-data"));
    let prefix = String::from_utf8_lossy(&prefix);
    assert!(prefix.find("fixture-policy").unwrap() < prefix.find("filename=\"long.wav\"").unwrap());
    assert!(tail.ends_with(b"--\r\n"));
    assert!(count > size as usize && count < size as usize + 4096);
    drop(runtime);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn sdk_file_request_streams_base64_past_old_media_and_request_body_limits() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let request = drain_request(&mut stream);
        reply(&mut stream, r#"{"output":{"text":"本地分块转写"}}"#);
        request
    });
    let source = r#"
export default () => ({ async invoke(request) {
  const sdk = globalThis.__sayitCreateSdkRuntime({ sources: ['bailian-speech-recognition'],
    capabilityOptions: { bailianSpeechRecognition: { apiBaseUrl: request.payload.url } } });
  try {
    return await sdk.capabilities.execute('bailian.speech-recognition.fun-asr-flash-2026-06-15',
      { audio: {kind: 'media-ref', ref: 'audio'}, timestamps: true });
  } finally { await sdk.dispose(); globalThis.__sayitDisposeRuntimeContext(); }
} });
"#;
    let (root, mut spec, profile) = fixture(source);
    spec.permissions.push("localNetwork".into());
    let file = root.join("long.wav");
    let size = 13 * 1024 * 1024;
    std::fs::File::create(&file).unwrap().set_len(size).unwrap();
    let mut sdk_bindings = bindings(Arc::new(Mutex::new(Vec::new())));
    sdk_bindings
        .credential_scopes
        .insert("speech-recognition".into());
    let runtime = JsProviderRuntime::create_with_sdk_bindings(
        spec,
        &profile,
        Duration::from_secs(5),
        Arc::new(AtomicBool::new(false)),
        HashMap::from([("audio".into(), file)]),
        sdk_bindings,
    )
    .unwrap();
    let result = runtime
        .call(
            "invoke",
            &json!({"payload":{"url":url}}),
            // Debug QuickJS 的 SDK Base64 编码明显慢于原生文件直传；本测试验证有界内存与字节完整性。
            Duration::from_secs(120),
        )
        .unwrap();
    assert_eq!(result["text"], "本地分块转写");
    assert_eq!(runtime.sdk_resource_counts(), (0, 0, 0));
    let (_, prefix, tail, count) = server.join().unwrap();
    assert!(String::from_utf8_lossy(&prefix).contains("data:audio/wav;base64,AAAA"));
    assert!(String::from_utf8_lossy(&tail).ends_with('}'));
    assert!(count > 16 * 1024 * 1024);
    drop(runtime);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn upload_preflight_preserves_size_errors_and_denies_unscoped_files_without_network() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let source = r#"
export default () => ({ async invoke(request) {
  const runtime = globalThis.__sayitCreateRuntimeContext();
  const options = { ref:'audio', fields:[], fileField:'file', maxBytes:1000000000, signal:new AbortController().signal };
  const result = {};
  try {
    try { await runtime.transport.uploadFile(request.payload.url, options); } catch (error) { result.size = {code:error.code, ...error.details}; }
    try { await runtime.transport.uploadFile(request.payload.url, {...options, ref:request.payload.path}); } catch (_) { result.scopeDenied = true; }
    const controller = new AbortController(); controller.abort();
    try { await runtime.transport.uploadFile(request.payload.url, {...options, signal:controller.signal}); } catch (error) { result.abort = error.name; }
    return result;
  } finally { globalThis.__sayitDisposeRuntimeContext(); }
} });
"#;
    let (root, spec, profile) = fixture(source);
    let file = root.join("oversize.wav");
    std::fs::File::create(&file)
        .unwrap()
        .set_len(1_000_000_001)
        .unwrap();
    let runtime = create_sdk_runtime(
        source,
        spec,
        &profile,
        Arc::new(AtomicBool::new(false)),
        HashMap::from([("audio".into(), file.clone())]),
        Arc::new(Mutex::new(Vec::new())),
    );
    let result = runtime
        .call(
            "invoke",
            &json!({"payload":{"url":url,"path":file}}),
            Duration::from_secs(5),
        )
        .unwrap();
    assert_eq!(
        result["size"],
        json!({"code":"media_too_large","actualBytes":1_000_000_001u64,"maxBytes":1_000_000_000u64})
    );
    assert_eq!(result["scopeDenied"], true);
    assert_eq!(result["abort"], "AbortError");
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
    assert_eq!(runtime.sdk_resource_counts(), (0, 0, 0));
    drop(runtime);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn signal_cancels_native_and_streaming_uploads_while_waiting_and_closes_iterator() {
    for mode in ["native", "stream"] {
        let (url, server) = spawn_stalled_http();
        let source = r#"
export default () => ({ async invoke(request) {
  const runtime = globalThis.__sayitCreateRuntimeContext();
  const controller = new AbortController();
  let closed = false;
  const timer = setTimeout(() => controller.abort(), 25);
  try {
    if (request.payload.mode === 'native') {
      await runtime.transport.uploadFile(request.payload.url, {ref:'audio', fields:[], fileField:'file', maxBytes:1000000, signal:controller.signal});
    } else {
      const body = (async function* () { try { yield new Uint8Array([1,2,3]); } finally { closed = true; } })();
      await runtime.transport.fetchStream(request.payload.url, {body, contentLength:3, signal:controller.signal});
    }
    throw Error('expected abort');
  } catch(error) { return {name:error.name, closed}; }
  finally { clearTimeout(timer); globalThis.__sayitDisposeRuntimeContext(); }
} });
"#;
        let (root, spec, profile) = fixture(source);
        let file = root.join("short.wav");
        std::fs::write(&file, b"fixture").unwrap();
        let runtime = create_sdk_runtime(
            source,
            spec,
            &profile,
            Arc::new(AtomicBool::new(false)),
            HashMap::from([("audio".into(), file)]),
            Arc::new(Mutex::new(Vec::new())),
        );
        let result = runtime
            .call(
                "invoke",
                &json!({"payload":{"url":url,"mode":mode}}),
                Duration::from_secs(3),
            )
            .unwrap();
        assert_eq!(result["name"], "AbortError", "{mode}");
        if mode == "stream" {
            assert_eq!(result["closed"], true);
        }
        assert_eq!(runtime.sdk_resource_counts(), (0, 0, 0));
        server.join().unwrap();
        drop(runtime);
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn sdk_async_file_runs_policy_native_upload_submit_poll_and_result() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        for step in 0..5 {
            let (mut stream, _) = listener.accept().unwrap();
            let (headers, prefix, _, count) = drain_request(&mut stream);
            let body = match step {
                0 => {
                    assert!(headers.starts_with("GET /uploads?"));
                    json!({"data":{"upload_host":"https://fixture-oss.invalid/upload","upload_dir":"fixture",
                        "max_file_size_mb":"1000","oss_access_key_id":"fixture-access","signature":"fixture-signature",
                        "policy":"fixture-policy","x_oss_object_acl":"private","x_oss_forbid_overwrite":"true"}})
                }
                1 => {
                    assert!(headers.starts_with("POST /upload"));
                    assert!(count > 12 * 1024 * 1024);
                    assert!(String::from_utf8_lossy(&prefix).contains("fixture-policy"));
                    json!({})
                }
                2 => {
                    assert!(headers.starts_with("POST /services/audio/asr/transcription"));
                    let body: Value = serde_json::from_slice(&prefix).unwrap();
                    assert_eq!(body["model"], "qwen-audio-3.1-asr-flash-filetrans");
                    assert_eq!(body["parameters"]["channel_id"], json!([0]));
                    assert_eq!(body["parameters"]["vocabulary"], json!({"说吧":3}));
                    assert!(body["input"]["file_urls"][0]
                        .as_str()
                        .unwrap()
                        .starts_with("oss://fixture/"));
                    json!({"output":{"task_id":"fixture-task","task_status":"PENDING"}})
                }
                3 => {
                    assert!(headers.starts_with("GET /tasks/fixture-task"));
                    json!({"output":{"task_id":"fixture-task","task_status":"SUCCEEDED",
                        "results":[{"transcription_url":"https://fixture-oss.invalid/result"} ]}})
                }
                _ => {
                    assert!(headers.starts_with("GET /result"));
                    json!({"transcripts":[{"text":"完整本地转写","sentences":[{
                        "text":"完整本地转写","speaker_id":2,"begin_time":0,"end_time":1000}]}]})
                }
            };
            reply(&mut stream, &body.to_string());
        }
    });
    let source = r#"
export default () => ({ async invoke(request) {
  const host = globalThis.__sayitCreateRuntimeContext();
  const fetch = host.transport.fetch;
  host.transport.fetch = (url, options) => fetch(url === 'https://fixture-oss.invalid/result' ? `${request.payload.url}/result` : url, options);
  const upload = host.transport.uploadFile;
  host.transport.uploadFile = (url, options) => {
    if (url !== 'https://fixture-oss.invalid/upload') throw Error('unexpected upload target');
    return upload(`${request.payload.url}/upload`, options);
  };
  host.media.read = host.media.describe = host.media.readChunk = () => {throw Error('must use native upload');};
  const sdk = globalThis.__sayitAiSdkCapabilities.createSayItCapabilityRuntime(host, {
    sources:['bailian-speech-recognition'], bailianSpeechRecognition: {
      apiBaseUrl:request.payload.url, uploadBaseUrl:request.payload.url, pollIntervalMs:1, maxPollingMs:2000,
    },
  });
  try {
    return await sdk.execute('bailian.speech-recognition.qwen-audio-3.1-asr-flash-filetrans', {
      audio:{kind:'media-ref',ref:'audio'}, timestamps:true,
      options:{channelId:0, vocabulary:{'说吧':3}, diarizationEnabled:true},
    });
  } finally {await sdk.dispose(); globalThis.__sayitDisposeRuntimeContext();}
} });
"#;
    let (root, mut spec, profile) = fixture(source);
    spec.permissions.push("localNetwork".into());
    let file = root.join("long.wav");
    std::fs::File::create(&file)
        .unwrap()
        .set_len(12 * 1024 * 1024)
        .unwrap();
    let mut sdk_bindings = bindings(Arc::new(Mutex::new(Vec::new())));
    sdk_bindings
        .credential_scopes
        .insert("speech-recognition".into());
    let runtime = JsProviderRuntime::create_with_sdk_bindings(
        spec,
        &profile,
        Duration::from_secs(5),
        Arc::new(AtomicBool::new(false)),
        HashMap::from([("audio".into(), file)]),
        sdk_bindings,
    )
    .unwrap();
    let result = runtime
        .call(
            "invoke",
            &json!({"payload":{"url":url}}),
            Duration::from_secs(10),
        )
        .unwrap();
    assert_eq!(result["text"], "完整本地转写");
    assert_eq!(result["segments"][0]["speakerId"], 2);
    assert_eq!(runtime.sdk_resource_counts(), (0, 0, 0));
    server.join().unwrap();
    drop(runtime);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn media_chunks_reject_file_changes_after_description() {
    let source = r#"
export default () => ({async invoke(request) {
  const runtime = globalThis.__sayitCreateRuntimeContext();
  if (request.payload.describe) return await runtime.media.describe('audio');
  try { return await runtime.media.readChunk('audio',0,5); }
  finally {globalThis.__sayitDisposeRuntimeContext();}
} });
"#;
    let (root, spec, profile) = fixture(source);
    let file = root.join("change.wav");
    std::fs::write(&file, b"12345").unwrap();
    let runtime = create_sdk_runtime(
        source,
        spec,
        &profile,
        Arc::new(AtomicBool::new(false)),
        HashMap::from([("audio".into(), file.clone())]),
        Arc::new(Mutex::new(Vec::new())),
    );
    runtime
        .call(
            "invoke",
            &json!({"payload":{"describe":true}}),
            Duration::from_secs(3),
        )
        .unwrap();
    std::fs::write(&file, b"123456").unwrap();
    let error = runtime
        .call("invoke", &json!({"payload":{}}), Duration::from_secs(3))
        .unwrap_err();
    assert!(error.contains("文件已发生变化"));
    drop(runtime);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn streamed_body_errors_close_iterators_and_release_uploads() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let source = r#"
export default () => ({async invoke(request) {
  const runtime = globalThis.__sayitCreateRuntimeContext();
  const results = [];
  try {
    for (const length of [2,4]) {
      let closed = false;
      const body = (async function* () {try {yield new Uint8Array([1,2,3]);} finally {closed=true;}})();
      try {await runtime.transport.fetchStream(request.payload.url, {body, contentLength:length});}
      catch(error) {results.push({closed, message:error.message});}
    }
    return results;
  } finally {globalThis.__sayitDisposeRuntimeContext();}
} });
"#;
    let (root, spec, profile) = fixture(source);
    let runtime = create_sdk_runtime(
        source,
        spec,
        &profile,
        Arc::new(AtomicBool::new(false)),
        HashMap::new(),
        Arc::new(Mutex::new(Vec::new())),
    );
    let result = runtime
        .call(
            "invoke",
            &json!({"payload":{"url":url}}),
            Duration::from_secs(3),
        )
        .unwrap();
    assert_eq!(result.as_array().unwrap().len(), 2);
    assert!(result
        .as_array()
        .unwrap()
        .iter()
        .all(|item| item["closed"] == true
            && item["message"].as_str().unwrap().contains("请求体长度")));
    assert_eq!(runtime.sdk_resource_counts(), (0, 0, 0));
    drop(runtime);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn streamed_upload_returns_redirect_without_replaying_body() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let target = TcpListener::bind("127.0.0.1:0").unwrap();
    target.set_nonblocking(true).unwrap();
    let redirect = format!("http://{}/unexpected", target.local_addr().unwrap());
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        assert_eq!(drain_request(&mut stream).3, 3);
        write!(stream, "HTTP/1.1 307 Temporary Redirect\r\nLocation: {redirect}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
    });
    let source = r#"
export default () => ({async invoke(request) {
  const runtime = globalThis.__sayitCreateRuntimeContext();
  try {
    const response = await runtime.transport.fetchStream(request.payload.url, {
      body:(async function*(){yield new Uint8Array([1,2,3]);})(), contentLength:3,
    });
    await response.text();
    return response.status;
  } finally {globalThis.__sayitDisposeRuntimeContext();}
} });
"#;
    let (root, spec, profile) = fixture(source);
    let runtime = create_sdk_runtime(
        source,
        spec,
        &profile,
        Arc::new(AtomicBool::new(false)),
        HashMap::new(),
        Arc::new(Mutex::new(Vec::new())),
    );
    assert_eq!(
        runtime
            .call(
                "invoke",
                &json!({"payload":{"url":url}}),
                Duration::from_secs(3)
            )
            .unwrap(),
        307
    );
    server.join().unwrap();
    assert_eq!(
        target.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
    assert_eq!(runtime.sdk_resource_counts(), (0, 0, 0));
    drop(runtime);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn early_http_rejection_stops_body_production_and_preserves_status() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut headers = Vec::new();
        while !headers.ends_with(b"\r\n\r\n") {
            let mut byte = [0];
            stream.read_exact(&mut byte).unwrap();
            headers.push(byte[0]);
        }
        stream.write_all(b"HTTP/1.1 413 Content Too Large\r\nContent-Length: 5\r\nConnection: close\r\n\r\nlimit").unwrap();
        // 先让客户端读到响应，再关闭未消费的请求体，避免 TCP reset 覆盖响应。
        thread::sleep(Duration::from_millis(100));
    });
    let source = r#"
export default () => ({async invoke(request) {
  const runtime = globalThis.__sayitCreateRuntimeContext();
  let produced = 0, closed = false;
  const body = (async function*(){try {while(produced < 1000){produced++; yield new Uint8Array(65536);}} finally {closed=true;}})();
  try {
    const response = await runtime.transport.fetchStream(request.payload.url, {body, contentLength:65536000});
    return {status:response.status, text:await response.text(), produced, closed};
  } finally {globalThis.__sayitDisposeRuntimeContext();}
} });
"#;
    let (root, spec, profile) = fixture(source);
    let runtime = create_sdk_runtime(
        source,
        spec,
        &profile,
        Arc::new(AtomicBool::new(false)),
        HashMap::new(),
        Arc::new(Mutex::new(Vec::new())),
    );
    let result = runtime
        .call(
            "invoke",
            &json!({"payload":{"url":url}}),
            Duration::from_secs(3),
        )
        .unwrap();
    assert_eq!(result["status"], 413);
    assert_eq!(result["text"], "limit");
    assert_eq!(result["closed"], true);
    assert!(result["produced"].as_u64().unwrap() < 1000);
    assert_eq!(runtime.sdk_resource_counts(), (0, 0, 0));
    server.join().unwrap();
    drop(runtime);
    std::fs::remove_dir_all(root).unwrap();
}
