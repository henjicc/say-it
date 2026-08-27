use std::collections::{HashMap, HashSet};

use rquickjs::{CaughtError, Context, Function, Runtime};
use serde::{Deserialize, Serialize};

use super::registry::ModelInfo;

pub const PLUGIN_CAPABILITY_PROTOCOL: &str = "sdk-capability";

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginSourceManifest {
    pub namespace: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginCapabilityManifest {
    pub module_id: String,
    pub kind: String,
    pub provider_ids: Vec<String>,
    pub model_id: String,
    #[serde(default)]
    pub operations: Vec<String>,
    #[serde(default)]
    pub features: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub execution_modes: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub accepted_input_kinds: Vec<String>,
    #[serde(default)]
    pub model_discovery: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginModelManifest {
    pub id: String,
    pub label: String,
    pub provider_id: String,
    pub capability_id: String,
    #[serde(default)]
    pub is_default_realtime: bool,
    #[serde(default)]
    pub is_default_file: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ValidatedDescriptor {
    id: String,
    kind: String,
    provider_ids: Vec<String>,
    model_id: String,
    #[allow(dead_code)]
    operations: Vec<String>,
    features: Vec<String>,
    #[allow(dead_code)]
    tags: Vec<String>,
    execution_modes: Vec<String>,
}

/// 语义校验只调用发布包内的 CapabilityClient 注册入口；Rust 不复制 SDK 的
/// stable-id、kind、坐标、operations 或 executionModes 规则。
pub fn validate_registry_with_sdk(
    plugins: &[(&str, &str, &[PluginCapabilityManifest])],
) -> Result<(), String> {
    let capability_payload = plugins
        .iter()
        .map(|(plugin_id, namespace, capabilities)| {
            serde_json::json!({
                "pluginId": plugin_id,
                "sourceNamespace": namespace,
                "capabilities": capabilities.iter().filter(|item| item.kind != "llm").collect::<Vec<_>>(),
            })
        })
        .collect::<Vec<_>>();
    let llm_payload = plugins
        .iter()
        .map(|(plugin_id, namespace, capabilities)| {
            serde_json::json!({
                "pluginId": plugin_id,
                "sourceNamespace": namespace,
                "capabilities": capabilities.iter().filter(|item| item.kind == "llm").collect::<Vec<_>>(),
            })
        })
        .collect::<Vec<_>>();
    let runtime = Runtime::new().map_err(|error| error.to_string())?;
    runtime.set_memory_limit(32 * 1024 * 1024);
    let context = Context::full(&runtime).map_err(|error| error.to_string())?;
    context.with(|ctx| -> Result<(), String> {
        let host_call = Function::new(ctx.clone(), |_operation: String, _payload: String| {
            r#"{"ok":false,"error":"manifest validation 禁止宿主调用"}"#.to_string()
        })
        .map_err(|error| error.to_string())?;
        ctx.globals()
            .set("__sayitHostCall", host_call)
            .map_err(|error| error.to_string())?;
        ctx.eval::<(), _>(super::sdk_runtime::QUICKJS_RUNTIME_BOOTSTRAP)
            .map_err(|error| error.to_string())?;
        ctx.eval::<(), _>(super::sdk_runtime::AI_SDK_CAPABILITIES_BUNDLE)
            .map_err(|error| error.to_string())?;
        let bundle: rquickjs::Object = ctx
            .globals()
            .get("__sayitAiSdkCapabilities")
            .map_err(|error| error.to_string())?;
        let validate: Function = bundle
            .get("validateSayItPluginCapabilityRegistry")
            .map_err(|_| "AI SDK capability bundle 缺少插件 v5 validator".to_string())?;
        let json = serde_json::to_string(&capability_payload).map_err(|error| error.to_string())?;
        validate.call::<_, String>((json,)).map_err(|error| {
            format!(
                "SDK capability 注册校验失败：{}",
                CaughtError::from_error(&ctx, error)
            )
        })?;
        ctx.eval::<(), _>(super::sdk_runtime::AI_SDK_GROQ_BUNDLE)
            .map_err(|error| error.to_string())?;
        ctx.eval::<(), _>(super::sdk_runtime::AI_SDK_LLM_MODULES_BUNDLE)
            .map_err(|error| error.to_string())?;
        let groq: rquickjs::Object = ctx
            .globals()
            .get("__sayitAiSdkGroq")
            .map_err(|error| error.to_string())?;
        let builtin_descriptors: Function = groq
            .get("sayItGroqModuleDescriptorJson")
            .map_err(|_| "AI SDK Groq bundle 缺少 module descriptor".to_string())?;
        let builtin_json: String = builtin_descriptors
            .call(())
            .map_err(|error| error.to_string())?;
        let llm_bundle: rquickjs::Object = ctx
            .globals()
            .get("__sayitAiSdkLlmModules")
            .map_err(|error| error.to_string())?;
        let validate_llm: Function = llm_bundle
            .get("validateSayItPluginLlmRegistry")
            .map_err(|_| "AI SDK LLM modules bundle 缺少插件 validator".to_string())?;
        let llm_json = serde_json::to_string(&llm_payload).map_err(|error| error.to_string())?;
        validate_llm
            .call::<_, String>((llm_json, builtin_json))
            .map_err(|error| {
                format!(
                    "SDK LLM module 注册校验失败：{}",
                    CaughtError::from_error(&ctx, error)
                )
            })?;
        Ok(())
    })
}

pub fn project_models(
    provider_id: &str,
    capabilities: &[PluginCapabilityManifest],
    models: &[PluginModelManifest],
) -> Result<Vec<ModelInfo>, String> {
    let executable_capabilities = capabilities
        .iter()
        .filter(|capability| capability.kind != "llm")
        .cloned()
        .collect::<Vec<_>>();
    let descriptors = validate_and_snapshot(&executable_capabilities)?;
    let by_id = descriptors
        .iter()
        .map(|descriptor| (descriptor.id.as_str(), descriptor))
        .collect::<HashMap<_, _>>();
    let mut model_ids = HashSet::new();
    let mut result = Vec::with_capacity(models.len());
    for model in models {
        if model.provider_id != provider_id {
            return Err(format!(
                "模型 {} 的 providerId 必须为 {provider_id}",
                model.id
            ));
        }
        if !model_ids.insert(model.id.as_str()) {
            return Err(format!("模型 ID 重复：{}", model.id));
        }
        let manifest_descriptor = capabilities
            .iter()
            .find(|descriptor| descriptor.module_id == model.capability_id)
            .ok_or_else(|| {
                format!(
                    "模型 {} 引用了不存在的 capabilityId：{}",
                    model.id, model.capability_id
                )
            })?;
        if manifest_descriptor.kind == "llm" {
            if manifest_descriptor.model_id != model.id
                || manifest_descriptor.provider_ids.as_slice() != [provider_id]
            {
                return Err(format!(
                    "模型 {} 与 capability {} 的 provider/model 坐标不一致",
                    model.id, manifest_descriptor.module_id
                ));
            }
            continue;
        }
        let descriptor = by_id
            .get(model.capability_id.as_str())
            .ok_or_else(|| format!("capability {} 未通过 SDK 校验", model.capability_id))?;
        if descriptor.model_id != model.id || descriptor.provider_ids.as_slice() != [provider_id] {
            return Err(format!(
                "模型 {} 与 capability {} 的 provider/model 坐标不一致",
                model.id, descriptor.id
            ));
        }
        let realtime = descriptor
            .execution_modes
            .iter()
            .any(|mode| mode == "realtime");
        let (category, scenes) = match descriptor.kind.as_str() {
            "speech-recognition" if realtime => (
                "realtime",
                vec!["dictationRealtime".into(), "subtitles".into()],
            ),
            "speech-recognition" => ("file", vec!["dictationFile".into(), "transcription".into()]),
            "translation" => ("translation", vec!["subtitleTranslation".into()]),
            other => {
                return Err(format!(
                    "capability {} 的 kind {} 不能投影为 ASR/翻译模型",
                    descriptor.id, other
                ))
            }
        };
        let feature = |name: &str| descriptor.features.iter().any(|value| value == name);
        result.push(ModelInfo {
            id: model.id.clone(),
            label: model.label.clone(),
            provider_id: model.provider_id.clone(),
            category: category.into(),
            protocol: PLUGIN_CAPABILITY_PROTOCOL.into(),
            capability_id: Some(descriptor.id.clone()),
            supports_vocabulary: feature("vocabulary"),
            supports_context: feature("context").then_some(true),
            supports_alignment_timestamps: feature("timestamps"),
            emits_partial_results: Some(realtime && feature("partial-results")),
            scenes,
            is_default_realtime: model.is_default_realtime,
            is_default_file: model.is_default_file,
        });
    }
    for descriptor in &descriptors {
        if !models
            .iter()
            .any(|model| model.capability_id == descriptor.id)
        {
            return Err(format!(
                "capability {} 没有对应的 models 条目",
                descriptor.id
            ));
        }
    }
    Ok(result)
}

fn validate_and_snapshot(
    capabilities: &[PluginCapabilityManifest],
) -> Result<Vec<ValidatedDescriptor>, String> {
    let json = serde_json::to_string(capabilities).map_err(|error| error.to_string())?;
    let runtime = Runtime::new().map_err(|error| error.to_string())?;
    runtime.set_memory_limit(32 * 1024 * 1024);
    let context = Context::full(&runtime).map_err(|error| error.to_string())?;
    context.with(|ctx| {
        let host_call = Function::new(ctx.clone(), |_operation: String, _payload: String| {
            r#"{"ok":false,"error":"manifest validation 禁止宿主调用"}"#.to_string()
        })
        .map_err(|error| error.to_string())?;
        ctx.globals()
            .set("__sayitHostCall", host_call)
            .map_err(|error| error.to_string())?;
        ctx.eval::<(), _>(super::sdk_runtime::QUICKJS_RUNTIME_BOOTSTRAP)
            .map_err(|error| error.to_string())?;
        ctx.eval::<(), _>(super::sdk_runtime::AI_SDK_CAPABILITIES_BUNDLE)
            .map_err(|error| error.to_string())?;
        let bundle: rquickjs::Object = ctx
            .globals()
            .get("__sayitAiSdkCapabilities")
            .map_err(|error| error.to_string())?;
        let validate: Function = bundle
            .get("validateSayItPluginCapabilityDefinitions")
            .map_err(|_| "AI SDK capability bundle 缺少插件 v5 validator".to_string())?;
        let value: String = validate
            .call((json, "manifest-validation"))
            .map_err(|error| {
                format!(
                    "SDK capability 定义校验失败：{}",
                    CaughtError::from_error(&ctx, error)
                )
            })?;
        serde_json::from_str(&value).map_err(|error| error.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capability(module_id: &str) -> PluginCapabilityManifest {
        PluginCapabilityManifest {
            module_id: module_id.into(),
            kind: "speech-recognition".into(),
            provider_ids: vec!["demo".into()],
            model_id: "demo-live".into(),
            operations: vec!["speech-recognition".into(), "speech-to-text".into()],
            features: vec!["streaming".into(), "partial-results".into()],
            tags: vec![],
            execution_modes: vec!["realtime".into()],
            accepted_input_kinds: vec![],
            model_discovery: false,
            context_window: None,
            max_output_tokens: None,
        }
    }

    fn llm_capability(
        module_id: &str,
        provider_id: &str,
        model_id: &str,
    ) -> PluginCapabilityManifest {
        PluginCapabilityManifest {
            module_id: module_id.into(),
            kind: "llm".into(),
            provider_ids: vec![provider_id.into()],
            model_id: model_id.into(),
            operations: vec!["chat".into(), "discover-models".into()],
            features: vec!["reasoning".into(), "usage".into(), "sampling".into()],
            tags: vec!["plugin".into()],
            execution_modes: vec!["request-response".into(), "event-stream".into()],
            accepted_input_kinds: vec!["text".into(), "image".into()],
            model_discovery: true,
            context_window: Some(32_768),
            max_output_tokens: Some(4_096),
        }
    }

    #[test]
    fn projects_sdk_descriptor_to_application_model() {
        let definitions = [capability("demo.speech-recognition.demo-live")];
        let models = [PluginModelManifest {
            id: "demo-live".into(),
            label: "Demo".into(),
            provider_id: "demo".into(),
            capability_id: definitions[0].module_id.clone(),
            is_default_realtime: false,
            is_default_file: false,
        }];
        let projected = project_models("demo", &definitions, &models).unwrap();
        assert_eq!(projected[0].category, "realtime");
        assert_eq!(projected[0].scenes, ["dictationRealtime", "subtitles"]);
        assert!(projected[0].emits_partial_results());
    }

    #[test]
    fn sdk_validator_rejects_duplicate_coordinates() {
        let first = capability("demo.first");
        let mut second = first.clone();
        second.module_id = "demo.second".into();
        let error = validate_registry_with_sdk(&[("demo", "demo", &[first, second])]).unwrap_err();
        assert!(error.contains("demo.first"), "{error}");
        assert!(error.contains("demo.second"), "{error}");
    }

    #[test]
    fn llm_validator_accepts_future_input_kinds_and_rejects_missing_text() {
        let valid = llm_capability("demo.llm.chat", "demo", "demo-chat");
        validate_registry_with_sdk(&[("demo", "demo", std::slice::from_ref(&valid))]).unwrap();
        let mut invalid = valid;
        invalid.accepted_input_kinds = vec!["image".into()];
        let error = validate_registry_with_sdk(&[("demo", "demo", &[invalid])]).unwrap_err();
        assert!(error.contains("必须接受 text 输入"), "{error}");
    }

    #[test]
    fn llm_validator_rejects_builtin_groq_coordinates() {
        let conflict = llm_capability("demo.llm.groq-conflict", "groq", "openai/gpt-oss-20b");
        let error = validate_registry_with_sdk(&[("demo", "demo", &[conflict])]).unwrap_err();
        assert!(error.contains("groq.chat.openai/gpt-oss-20b"), "{error}");
        assert!(error.contains("demo.llm.groq-conflict"), "{error}");
    }

    #[test]
    fn llm_validator_rejects_cross_plugin_model_coordinate_conflict() {
        let first = llm_capability("first.llm.chat", "shared", "chat");
        let second = llm_capability("second.llm.chat", "shared", "chat");
        let error = validate_registry_with_sdk(&[
            ("first", "first", &[first]),
            ("second", "second", &[second]),
        ])
        .unwrap_err();
        assert!(error.contains("first.llm.chat"), "{error}");
        assert!(error.contains("second.llm.chat"), "{error}");
    }
}
