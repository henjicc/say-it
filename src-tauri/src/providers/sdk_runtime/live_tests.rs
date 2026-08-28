//! 默认禁用的真实供应商最小付费验收。
//!
//! 该入口只能在显式 `--ignored`、`SAY_IT_ALLOW_PAID=1` 与
//! `SAY_IT_PAID_CONFIRM=9.18` 三道门同时满足时运行。输出仅包含结构化状态、计数和摘要。

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use serde_json::{json, Value};

use super::live_result::{error_class, outcome, AuditRecorder, LiveCaseResult};
use super::online::{BuiltinSdkRuntime, BuiltinSdkScope};
use crate::providers::credential_store::{CredentialKey, CredentialStore, CredentialStoreHandle};
use crate::providers::{bailian_profile, groq_llm_profile, ProviderProfile};

const ASR_CATALOG: &str = include_str!("../../../../shared/asr-models.json");

#[derive(Clone)]
struct MissingCredentials;

impl CredentialStore for MissingCredentials {
    fn get(&self, _key: &CredentialKey) -> Result<Option<String>, String> {
        Ok(None)
    }

    fn set(&self, _key: &CredentialKey, _value: &str) -> Result<(), String> {
        unreachable!("missing credential fixture never writes")
    }

    fn delete(&self, _key: &CredentialKey) -> Result<(), String> {
        unreachable!("missing credential fixture never deletes")
    }
}

fn execute_capability(
    recorder: &AuditRecorder,
    model: &str,
    protocol: &str,
    source: &str,
    input: Value,
    inputs: HashMap<String, PathBuf>,
    scope: BuiltinSdkScope,
) -> LiveCaseResult {
    let started = Instant::now();
    let request_id = format!("live-{}-{}", source, uuid::Uuid::new_v4());
    let profile = bailian_profile();
    let runtime = BuiltinSdkRuntime::create_for_live_test(
        &profile,
        CredentialStoreHandle::default(),
        scope,
        request_id.clone(),
        Arc::new(AtomicBool::new(false)),
        inputs,
        Arc::new(recorder.clone()),
    );
    let Ok(runtime) = runtime else {
        return outcome(
            source,
            model,
            protocol,
            started,
            runtime.map(|_| Value::Null),
            vec![],
            (0, 0, 0),
        );
    };
    let result = runtime.execute_capability(
        source,
        &format!(
            "bailian.{}.{model}",
            if source == "asr" {
                "speech-recognition"
            } else {
                source
            }
        ),
        input,
        &request_id,
    );
    let events = runtime.take_events();
    let counts = runtime.resource_counts();
    outcome(source, model, protocol, started, result, events, counts)
}

fn run_asr(recorder: &AuditRecorder, model: &str, protocol: &str, audio: &Path) -> LiveCaseResult {
    execute_capability(
        recorder,
        model,
        protocol,
        "asr",
        json!({
            "audio": { "kind": "media-ref", "ref": "input-audio" },
            "language": "zh",
            "timestamps": true,
            "options": {},
        }),
        HashMap::from([("input-audio".into(), audio.to_path_buf())]),
        BuiltinSdkScope::SpeechRecognition,
    )
}

fn run_translation(recorder: &AuditRecorder, model: &str) -> LiveCaseResult {
    execute_capability(
        recorder,
        model,
        "qwen-mt-streaming",
        "translation",
        json!({
            "source": "你好",
            "sourceLanguage": "Chinese",
            "targetLanguage": "English",
            "options": { "stream": true },
        }),
        HashMap::new(),
        BuiltinSdkScope::Translation,
    )
}

fn run_realtime(
    recorder: &AuditRecorder,
    model: &str,
    protocol: &str,
    pcm: &[u8],
) -> LiveCaseResult {
    let started = Instant::now();
    let request_id = format!("live-realtime-{}", uuid::Uuid::new_v4());
    let profile = bailian_profile();
    let runtime = BuiltinSdkRuntime::create_for_live_test(
        &profile,
        CredentialStoreHandle::default(),
        BuiltinSdkScope::SpeechRecognition,
        request_id.clone(),
        Arc::new(AtomicBool::new(false)),
        HashMap::new(),
        Arc::new(recorder.clone()),
    );
    let Ok(runtime) = runtime else {
        return outcome(
            "realtime-asr",
            model,
            protocol,
            started,
            runtime.map(|_| Value::Null),
            vec![],
            (0, 0, 0),
        );
    };
    let module_id = format!("bailian.speech-recognition.{model}");
    let result = runtime
        .realtime_start(
            &module_id,
            json!({
                "mediaType": "audio/pcm",
                "sampleRateHz": 16000,
                "channels": 1,
                "language": "zh",
                "options": { "format": "pcm" },
            }),
            &request_id,
        )
        .and_then(|()| {
            for chunk in pcm.chunks(3_200) {
                runtime.realtime_audio(chunk.to_vec())?;
                runtime.dispatch_host_events()?;
            }
            runtime.realtime_finish()
        });
    if result.is_err() {
        let _ = runtime.realtime_stop();
    }
    let events = runtime.take_events();
    let counts = runtime.resource_counts();
    outcome(
        "realtime-asr",
        model,
        protocol,
        started,
        result,
        events,
        counts,
    )
}

fn run_groq(recorder: &AuditRecorder) -> LiveCaseResult {
    let started = Instant::now();
    let request_id = format!("live-groq-{}", uuid::Uuid::new_v4());
    let mut profile: ProviderProfile = groq_llm_profile();
    profile.config["model"] = json!("openai/gpt-oss-20b");
    let runtime = BuiltinSdkRuntime::create_for_live_test(
        &profile,
        CredentialStoreHandle::default(),
        BuiltinSdkScope::Groq,
        request_id.clone(),
        Arc::new(AtomicBool::new(false)),
        HashMap::new(),
        Arc::new(recorder.clone()),
    );
    let Ok(runtime) = runtime else {
        return outcome(
            "groq-stream",
            "openai/gpt-oss-20b",
            "openai-compatible-sse",
            started,
            runtime.map(|_| Value::Null),
            vec![],
            (0, 0, 0),
        );
    };
    let discovery = runtime.discover_groq();
    let has_model = discovery.as_ref().is_ok_and(|models| {
        models.as_array().is_some_and(|models| {
            models.iter().any(|model| {
                model.get("modelId").and_then(Value::as_str) == Some("openai/gpt-oss-20b")
            })
        })
    });
    let result = if has_model {
        runtime.run_groq(
            json!({
                "modelId": "openai/gpt-oss-20b",
                "messages": [{ "role": "user", "content": "只回答一个字：好" }],
                "capabilities": { "reasoning": true },
                "reasoning": { "enabled": true, "effort": "low" },
                "policy": { "maxTokens": 32 },
            }),
            &request_id,
            true,
        )
    } else {
        Err(discovery
            .err()
            .unwrap_or_else(|| "Groq 模型发现未返回默认模型".into()))
    };
    let events = runtime.take_events();
    let counts = runtime.resource_counts();
    outcome(
        "groq-stream",
        "openai/gpt-oss-20b",
        "openai-compatible-sse",
        started,
        result,
        events,
        counts,
    )
}

fn run_pre_cancel(recorder: &AuditRecorder) -> LiveCaseResult {
    let started = Instant::now();
    let request_id = format!("live-cancel-{}", uuid::Uuid::new_v4());
    let cancelled = Arc::new(AtomicBool::new(false));
    let runtime = BuiltinSdkRuntime::create_for_live_test(
        &groq_llm_profile(),
        CredentialStoreHandle::default(),
        BuiltinSdkScope::Groq,
        request_id.clone(),
        cancelled.clone(),
        HashMap::new(),
        Arc::new(recorder.clone()),
    );
    let Ok(runtime) = runtime else {
        return outcome(
            "pre-cancel",
            "openai/gpt-oss-20b",
            "host-cancel",
            started,
            runtime.map(|_| Value::Null),
            vec![],
            (0, 0, 0),
        );
    };
    cancelled.store(true, Ordering::Relaxed);
    let result = runtime.run_groq(json!({}), &request_id, true);
    let was_cancelled = result.is_err();
    let events = runtime.take_events();
    let counts = runtime.resource_counts();
    let mut result = outcome(
        "pre-cancel",
        "openai/gpt-oss-20b",
        "host-cancel",
        started,
        result,
        events,
        counts,
    );
    if was_cancelled {
        result.error_class = Some("cancelled");
    }
    result.success = was_cancelled && counts == (0, 0, 0);
    result
}

fn assert_audit_logs_are_safe(recorder: &AuditRecorder) {
    let records = recorder.0.lock().expect("audit recorder lock");
    let serialized = serde_json::to_string(&*records).expect("serialize audit records");
    let normalized = serialized.to_ascii_lowercase();
    for forbidden in ["authorization", "bearer ", "apikey", "api_key", "base64"] {
        assert!(
            !normalized.contains(forbidden),
            "结构化日志包含禁止字段或载荷标识：{forbidden}"
        );
    }
}

fn live_case_enabled(model_or_case: &str) -> bool {
    std::env::var("SAY_IT_LIVE_CASES")
        .ok()
        .map(|cases| {
            cases
                .split(',')
                .map(str::trim)
                .any(|case| case == model_or_case)
        })
        .unwrap_or(true)
}

fn record_live_result(results: &mut Vec<LiveCaseResult>, result: LiveCaseResult) {
    println!(
        "SAY_IT_LIVE_RESULT {}",
        serde_json::to_string(&result).expect("serialize live result")
    );
    results.push(result);
}

#[test]
fn live_catalog_maps_all_nine_bailian_asr_models() {
    let catalog: Vec<Value> = serde_json::from_str(ASR_CATALOG).expect("ASR catalog");
    let online = catalog
        .iter()
        .filter(|model| model.get("providerId").and_then(Value::as_str) == Some("bailian"))
        .collect::<Vec<_>>();
    assert_eq!(online.len(), 9);
    let mut protocols = BTreeMap::<String, usize>::new();
    for model in online {
        let protocol = model
            .get("protocol")
            .and_then(Value::as_str)
            .expect("online ASR protocol");
        *protocols.entry(protocol.into()).or_default() += 1;
    }
    assert_eq!(protocols.get("dashscope-duplex"), Some(&2));
    assert_eq!(protocols.get("qwen-realtime"), Some(&2));
    assert_eq!(protocols.get("file-sync-funasr-flash"), Some(&1));
    assert_eq!(protocols.get("file-sync-qwen"), Some(&2));
    assert_eq!(protocols.get("file-async-oss"), Some(&2));
}

#[test]
fn classifies_missing_bailian_credential_without_network() {
    let request_id = "missing-bailian-key";
    let runtime = BuiltinSdkRuntime::create_for_live_test(
        &bailian_profile(),
        CredentialStoreHandle::from_store(Arc::new(MissingCredentials)),
        BuiltinSdkScope::SpeechRecognition,
        request_id,
        Arc::new(AtomicBool::new(false)),
        HashMap::new(),
        Arc::new(AuditRecorder::default()),
    )
    .unwrap();
    let error = runtime
        .execute_capability(
            "asr",
            "bailian.speech-recognition.qwen3-asr-flash",
            json!({ "audio": { "kind": "bytes", "bytes": [0], "mediaType": "audio/wav" } }),
            request_id,
        )
        .unwrap_err();
    assert_eq!(error_class(&error), "missing_credential", "{error}");
    assert_eq!(runtime.resource_counts(), (0, 0, 0));
}

#[test]
#[ignore = "真实付费验收：还需 SAY_IT_ALLOW_PAID=1 与 SAY_IT_PAID_CONFIRM=9.18"]
fn paid_provider_minimum_acceptance() {
    assert_eq!(std::env::var("SAY_IT_ALLOW_PAID").as_deref(), Ok("1"));
    assert_eq!(std::env::var("SAY_IT_PAID_CONFIRM").as_deref(), Ok("9.18"));
    let audio = PathBuf::from(std::env::var("SAY_IT_LIVE_AUDIO").expect("缺少临时语音路径"));
    assert!(audio.is_file(), "临时语音文件不存在");
    let samples = crate::audio_prep::decode_to_mono_16k(audio.to_str().expect("UTF-8 path"))
        .expect("临时语音必须可解码");
    let pcm = crate::audio_prep::f32_to_i16(&samples)
        .into_iter()
        .flat_map(i16::to_le_bytes)
        .collect::<Vec<_>>();
    assert!(!pcm.is_empty(), "临时语音没有 PCM 数据");

    let recorder = AuditRecorder::default();
    let mut results = Vec::new();
    if live_case_enabled("qwen3-asr-flash") {
        record_live_result(
            &mut results,
            run_asr(&recorder, "qwen3-asr-flash", "short-audio-http", &audio),
        );
    }
    if live_case_enabled("fun-asr") {
        record_live_result(
            &mut results,
            run_asr(&recorder, "fun-asr", "file-async-upload-poll", &audio),
        );
    }
    if live_case_enabled("fun-asr-realtime") {
        record_live_result(
            &mut results,
            run_realtime(&recorder, "fun-asr-realtime", "fun-duplex-ws", &pcm),
        );
    }
    if live_case_enabled("qwen3-asr-flash-realtime") {
        record_live_result(
            &mut results,
            run_realtime(
                &recorder,
                "qwen3-asr-flash-realtime",
                "qwen-realtime-ws",
                &pcm,
            ),
        );
    }
    for model in ["qwen-mt-flash", "qwen-mt-plus", "qwen-mt-lite"] {
        if live_case_enabled(model) {
            record_live_result(&mut results, run_translation(&recorder, model));
        }
    }
    if live_case_enabled("openai/gpt-oss-20b") {
        record_live_result(&mut results, run_groq(&recorder));
    }
    if live_case_enabled("pre-cancel") {
        record_live_result(&mut results, run_pre_cancel(&recorder));
    }

    assert_audit_logs_are_safe(&recorder);
    assert!(!results.is_empty(), "SAY_IT_LIVE_CASES 没有匹配任何验收项");
    for result in &results {
        assert_eq!(result.resource_counts, (0, 0, 0));
    }
    let failures = results.iter().filter(|result| !result.success).count();
    assert_eq!(failures, 0, "真实供应商最小验收存在 {failures} 项失败");
}
