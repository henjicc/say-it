use crate::providers::{
    default_provider_id, find_profile, llm_models_from_config, normalize_llm_endpoint,
    normalize_settings, ProviderProfile,
};
use crate::state::RuntimeState;
use genai::adapter::AdapterKind;
use genai::chat::{ChatMessage, ChatOptions, ChatRequest, ReasoningEffort};
use genai::resolver::{AuthData, AuthResolver, Endpoint, ServiceTargetResolver};
use genai::{Client, ModelIden, ServiceTarget};
use tauri::State;
use tokio::time::{timeout, Duration};

const TEXT_PLACEHOLDER: &str = "{{text}}";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const SYSTEM_PROMPT: &str = "你是桌面听写应用的文本处理引擎。严格按照用户模板处理听写文本，只返回最终文本，不要解释、不要使用 Markdown 包裹。";

fn profile_value<'a>(profile: &'a ProviderProfile, key: &str) -> &'a str {
    profile
        .config
        .get(key)
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .trim()
}

pub(crate) fn render_prompt(template: &str, text: &str) -> Result<String, String> {
    let template = template.trim();
    if template.is_empty() {
        return Err("智能处理提示词不能为空".to_string());
    }
    if !template.contains(TEXT_PLACEHOLDER) {
        return Err(format!("智能处理提示词必须包含占位符 {TEXT_PLACEHOLDER}"));
    }
    Ok(template.replace(TEXT_PLACEHOLDER, text))
}

fn selected_profile(state: &RuntimeState) -> Result<ProviderProfile, String> {
    let settings = state
        .providers
        .lock()
        .map_err(|_| "大语言模型配置锁失败".to_string())?;
    let settings = normalize_settings(settings.clone());
    let provider_id = default_provider_id(&settings, "llm");
    find_profile(&settings, &provider_id)
        .filter(|profile| profile.enabled && profile.kind.starts_with("llm:"))
        .cloned()
        .ok_or_else(|| "请先在“设置 → 密钥与识别”中配置默认大语言模型".to_string())
}

fn client_and_model(profile: &ProviderProfile) -> Result<(Client, String), String> {
    let adapter = profile
        .kind
        .strip_prefix("llm:")
        .ok_or_else(|| "大语言模型供应商类型无效".to_string())?;
    let model = profile_value(profile, "model");
    if model.is_empty() {
        return Err(format!("请先为 {} 设置模型", profile.display_name));
    }
    let api_key = profile_value(profile, "apiKey").to_string();
    if api_key.is_empty() {
        return Err(format!("请先为 {} 设置 API Key", profile.display_name));
    }

    if adapter == "custom" {
        let endpoint = profile_value(profile, "endpoint").to_string();
        if !(endpoint.starts_with("https://") || endpoint.starts_with("http://")) {
            return Err("自定义大语言模型的接口地址无效".to_string());
        }
        let target_resolver = ServiceTargetResolver::from_resolver_fn(
            move |target: ServiceTarget| -> Result<ServiceTarget, genai::resolver::Error> {
                Ok(ServiceTarget {
                    endpoint: Endpoint::from_owned(normalize_llm_endpoint(&endpoint)),
                    auth: AuthData::from_single(api_key.clone()),
                    model: ModelIden::new(AdapterKind::OpenAI, target.model.model_name),
                })
            },
        );
        return Ok((
            Client::builder()
                .with_service_target_resolver(target_resolver)
                .build(),
            format!("openai::{model}"),
        ));
    }

    let auth_resolver = AuthResolver::from_resolver_fn(
        move |_model| -> Result<Option<AuthData>, genai::resolver::Error> {
            Ok(Some(AuthData::from_single(api_key.clone())))
        },
    );
    let resolved_model = if model.contains("::") {
        model.to_string()
    } else {
        format!("{adapter}::{model}")
    };
    Ok((
        Client::builder().with_auth_resolver(auth_resolver).build(),
        resolved_model,
    ))
}

fn chat_options(profile: &ProviderProfile) -> Result<ChatOptions, String> {
    let model_name = profile_value(profile, "model");
    let model = llm_models_from_config(&profile.config)
        .into_iter()
        .find(|model| model.name == model_name)
        .ok_or_else(|| format!("当前模型 {model_name} 的配置不存在"))?;
    let mut options = ChatOptions::default();
    if let Some(temperature) = model.temperature {
        if !(0.0..=2.0).contains(&temperature) {
            return Err("模型温度必须在 0 到 2 之间".to_string());
        }
        options = options.with_temperature(temperature);
    }
    if let Some(max_tokens) = model.max_tokens {
        if max_tokens == 0 {
            return Err("最大输出 Token 必须是正整数".to_string());
        }
        options = options.with_max_tokens(max_tokens);
    }
    let reasoning = match model.reasoning_effort.as_str() {
        "auto" | "" => None,
        "zero" => Some(ReasoningEffort::Zero),
        "low" => Some(ReasoningEffort::Low),
        "medium" => Some(ReasoningEffort::Medium),
        "high" => Some(ReasoningEffort::High),
        value => return Err(format!("不支持的推理强度：{value}")),
    };
    if let Some(reasoning) = reasoning {
        options = options.with_reasoning_effort(reasoning);
    }
    Ok(options)
}

pub(crate) async fn process_smart_text(
    state: &RuntimeState,
    text: &str,
    template: &str,
) -> Result<String, String> {
    if text.trim().is_empty() {
        return Ok(String::new());
    }
    let prompt = render_prompt(template, text)?;
    let profile = selected_profile(state)?;
    let (client, model) = client_and_model(&profile)?;
    let request = ChatRequest::default()
        .with_system(SYSTEM_PROMPT)
        .append_message(ChatMessage::user(prompt));
    let options = chat_options(&profile)?;
    let response = timeout(
        REQUEST_TIMEOUT,
        client.exec_chat(&model, request, Some(&options)),
    )
    .await
    .map_err(|_| "大语言模型处理超时（30 秒）".to_string())?
    .map_err(|error| format!("大语言模型调用失败：{error}"))?;
    let output = response
        .first_text()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "大语言模型没有返回文本".to_string())?;
    Ok(output.to_string())
}

#[tauri::command]
pub(crate) async fn preview_smart_text(
    text: String,
    prompt: String,
    state: State<'_, RuntimeState>,
) -> Result<String, String> {
    process_smart_text(&state, &text, &prompt).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_prompt_replaces_every_text_placeholder() {
        assert_eq!(
            render_prompt("整理：{{text}}\n原文：{{text}}", "你好").unwrap(),
            "整理：你好\n原文：你好"
        );
    }

    #[test]
    fn render_prompt_requires_placeholder() {
        assert!(render_prompt("帮我整理", "你好")
            .unwrap_err()
            .contains(TEXT_PLACEHOLDER));
    }

    #[test]
    fn legacy_model_uses_existing_temperature_default() {
        let profile = ProviderProfile {
            id: "test".into(),
            kind: "llm:groq".into(),
            display_name: "Test".into(),
            auth_kind: "api-key".into(),
            capabilities: vec!["llm".into()],
            enabled: true,
            config: serde_json::json!({"model": "demo"}),
            config_fields: vec![],
            actions: vec![],
        };
        let options = chat_options(&profile).unwrap();
        assert_eq!(options.temperature, Some(0.1));
        assert!(options.reasoning_effort.is_none());
    }

    #[test]
    fn model_options_apply_reasoning_temperature_and_max_tokens() {
        let profile = ProviderProfile {
            id: "test".into(),
            kind: "llm:groq".into(),
            display_name: "Test".into(),
            auth_kind: "api-key".into(),
            capabilities: vec!["llm".into()],
            enabled: true,
            config: serde_json::json!({
                "model": "demo",
                "models": [{
                    "name": "demo",
                    "source": "remote",
                    "availability": "available",
                    "reasoningEffort": "high",
                    "temperature": null,
                    "maxTokens": 512
                }]
            }),
            config_fields: vec![],
            actions: vec![],
        };
        let options = chat_options(&profile).unwrap();
        assert_eq!(options.temperature, None);
        assert_eq!(options.max_tokens, Some(512));
        assert!(matches!(
            options.reasoning_effort,
            Some(ReasoningEffort::High)
        ));
    }

    #[test]
    fn invalid_model_options_are_rejected() {
        let mut profile = crate::providers::groq_llm_profile();
        profile.config["models"][0]["temperature"] = serde_json::json!(2.5);
        assert!(chat_options(&profile).unwrap_err().contains("0 到 2"));
    }
}
