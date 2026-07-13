use super::{alibabacloud, ProviderProfile};
pub use alibabacloud::{
    HotwordEntry, TranscriptionParams, TranscriptionResult, TranscriptionTaskStatus,
};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapabilityError {
    pub provider_id: String,
    pub capability: &'static str,
    pub message: String,
}
impl std::fmt::Display for CapabilityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}
fn unsupported(profile: &ProviderProfile, capability: &'static str) -> CapabilityError {
    CapabilityError {
        provider_id: profile.id.clone(),
        capability,
        message: format!("供应商 {} 不支持 {capability} 能力", profile.display_name),
    }
}
fn api_key(profile: &ProviderProfile) -> Result<String, String> {
    let key = profile
        .config
        .get("apiKey")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    if key.is_empty() {
        Err("请先在设置中填写阿里云百炼 API Key".into())
    } else {
        Ok(key.into())
    }
}

#[derive(Clone)]
pub enum FileRecognitionProvider {
    AlibabaCloud {
        api_key: String,
        vocabulary_ids: HashMap<String, String>,
        hotwords: Vec<HotwordEntry>,
    },
}
pub fn file_recognition_for(
    profile: &ProviderProfile,
) -> Result<FileRecognitionProvider, CapabilityError> {
    match profile.kind.as_str() {
        "alibabacloud-funasr" => Ok(FileRecognitionProvider::AlibabaCloud {
            api_key: api_key(profile).map_err(|message| CapabilityError {
                provider_id: profile.id.clone(),
                capability: "fileRecognition",
                message,
            })?,
            vocabulary_ids: profile
                .config
                .get("vocabularyIds")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default(),
            hotwords: profile
                .config
                .get("hotwords")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default(),
        }),
        _ => Err(unsupported(profile, "fileRecognition")),
    }
}
impl FileRecognitionProvider {
    pub fn uses_async_task(&self, model: &str) -> bool {
        alibabacloud::uses_async_transcription_task(model)
    }
    pub async fn recognize_short(
        &self,
        path: &str,
        params: &TranscriptionParams,
    ) -> Result<TranscriptionResult, String> {
        match self {
            Self::AlibabaCloud {
                api_key, hotwords, ..
            } => alibabacloud::recognize_short_audio(api_key, path, params, hotwords).await,
        }
    }
    pub async fn upload(&self, model: &str, path: &str) -> Result<String, String> {
        match self {
            Self::AlibabaCloud { api_key, .. } => {
                alibabacloud::upload_for_model(api_key, model, path).await
            }
        }
    }
    pub async fn submit(
        &self,
        model: &str,
        url: &str,
        params: &TranscriptionParams,
    ) -> Result<String, String> {
        match self {
            Self::AlibabaCloud {
                api_key,
                vocabulary_ids,
                ..
            } => {
                alibabacloud::submit_transcription_task(
                    api_key,
                    url,
                    params,
                    vocabulary_ids
                        .get(model)
                        .map(String::as_str)
                        .unwrap_or_default(),
                )
                .await
            }
        }
    }
    pub async fn query(&self, id: &str) -> Result<TranscriptionTaskStatus, String> {
        match self {
            Self::AlibabaCloud { api_key, .. } => {
                alibabacloud::query_transcription_task(api_key, id).await
            }
        }
    }
    pub async fn fetch(&self, url: &str) -> Result<TranscriptionResult, String> {
        match self {
            Self::AlibabaCloud { .. } => alibabacloud::fetch_transcription_result(url).await,
        }
    }
}

#[derive(Clone)]
pub enum TranslationProvider {
    AlibabaCloud { api_key: String },
}
pub fn translation_for(profile: &ProviderProfile) -> Result<TranslationProvider, CapabilityError> {
    match profile.kind.as_str() {
        "alibabacloud-funasr" => Ok(TranslationProvider::AlibabaCloud {
            api_key: api_key(profile).map_err(|message| CapabilityError {
                provider_id: profile.id.clone(),
                capability: "translation",
                message,
            })?,
        }),
        _ => Err(unsupported(profile, "translation")),
    }
}
impl TranslationProvider {
    pub async fn translate_streaming<F>(
        &self,
        model: &str,
        text: &str,
        source: &str,
        target: &str,
        on_delta: F,
    ) -> Result<String, String>
    where
        F: FnMut(&str) + Send,
    {
        match self {
            Self::AlibabaCloud { api_key } => {
                alibabacloud::translate_streaming(api_key, model, text, source, target, on_delta)
                    .await
            }
        }
    }
}

#[derive(Clone)]
pub enum CustomizationProvider {
    AlibabaCloud { api_key: String },
}
pub fn customization_for(
    profile: &ProviderProfile,
) -> Result<CustomizationProvider, CapabilityError> {
    match profile.kind.as_str() {
        "alibabacloud-funasr" => Ok(CustomizationProvider::AlibabaCloud {
            api_key: profile
                .config
                .get("apiKey")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim()
                .to_string(),
        }),
        _ => Err(unsupported(profile, "customization")),
    }
}
impl CustomizationProvider {
    pub fn ensure_ready(&self) -> Result<(), String> {
        match self {
            Self::AlibabaCloud { api_key } if api_key.is_empty() => {
                Err("请先在设置中填写阿里云百炼 API Key".into())
            }
            _ => Ok(()),
        }
    }
    pub fn targets(&self) -> &'static [(&'static str, &'static str)] {
        alibabacloud::VOCABULARY_TARGETS
    }
    pub async fn create(
        &self,
        model: &str,
        prefix: &str,
        words: &[HotwordEntry],
    ) -> Result<String, String> {
        match self {
            Self::AlibabaCloud { api_key } => {
                alibabacloud::create_vocabulary(api_key, model, prefix, words).await
            }
        }
    }
    pub async fn update(&self, id: &str, words: &[HotwordEntry]) -> Result<(), String> {
        match self {
            Self::AlibabaCloud { api_key } => {
                alibabacloud::update_vocabulary(api_key, id, words).await
            }
        }
    }
    pub async fn delete(&self, id: &str) -> Result<(), String> {
        match self {
            Self::AlibabaCloud { api_key } => alibabacloud::delete_vocabulary(api_key, id).await,
        }
    }
    pub async fn list(&self, prefix: &str) -> Result<Vec<String>, String> {
        match self {
            Self::AlibabaCloud { api_key } => alibabacloud::list_vocabulary(api_key, prefix).await,
        }
    }
    pub async fn query(&self, id: &str) -> Result<Vec<HotwordEntry>, String> {
        match self {
            Self::AlibabaCloud { api_key } => alibabacloud::query_vocabulary(api_key, id).await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn fake() -> ProviderProfile {
        ProviderProfile {
            id: "fake".into(),
            kind: "fake-kind".into(),
            display_name: "Fake".into(),
            auth_kind: "none".into(),
            capabilities: vec!["asr".into()],
            enabled: true,
            config: json!({}),
        }
    }
    #[test]
    fn missing_capability_is_structured() {
        let e = match file_recognition_for(&fake()) {
            Err(e) => e,
            Ok(_) => panic!(),
        };
        assert_eq!(
            (e.provider_id.as_str(), e.capability),
            ("fake", "fileRecognition")
        );
    }
    #[test]
    fn registered_provider_exposes_real_capabilities() {
        let mut p = super::super::funasr_profile();
        p.config["apiKey"] = json!("test");
        assert!(file_recognition_for(&p).is_ok());
        assert!(translation_for(&p).is_ok());
        assert!(customization_for(&p).is_ok());
    }
}
