use std::collections::HashSet;
use std::sync::Arc;

use crate::providers::credential_store::{CredentialKey, CredentialStoreHandle};
use serde_json::Value;

pub mod online;

pub const QUICKJS_RUNTIME_BOOTSTRAP: &str = include_str!(concat!(
    env!("OUT_DIR"),
    "/sayit-ai-sdk-runtime/sayit-sdk-runtime-bootstrap.js"
));
pub const AI_SDK_CAPABILITIES_BUNDLE: &str = include_str!(concat!(
    env!("OUT_DIR"),
    "/sayit-ai-sdk-runtime/sayit-ai-sdk-capabilities.js"
));
pub const AI_SDK_GROQ_BUNDLE: &str = include_str!(concat!(
    env!("OUT_DIR"),
    "/sayit-ai-sdk-runtime/sayit-ai-sdk-groq.js"
));
pub const AI_SDK_LLM_MODULES_BUNDLE: &str = include_str!(concat!(
    env!("OUT_DIR"),
    "/sayit-ai-sdk-runtime/sayit-ai-sdk-llm-modules.js"
));
pub const AI_SDK_BOOTSTRAP: &str = include_str!(concat!(
    env!("OUT_DIR"),
    "/sayit-ai-sdk-runtime/sayit-ai-sdk-bootstrap.js"
));
#[cfg(test)]
pub const AI_SDK_BUNDLE_MANIFEST: &str = include_str!(concat!(
    env!("OUT_DIR"),
    "/sayit-ai-sdk-runtime/sayit-ai-sdk-manifest.json"
));

pub trait HostRuntimeRecorder: Send + Sync {
    fn record(&self, event: Value);
}

#[derive(Clone)]
pub struct SdkHostBindings {
    pub owner_id: String,
    pub provider_id: String,
    pub request_id: String,
    pub credential_scopes: HashSet<String>,
    pub credential_key: CredentialKey,
    pub credentials: CredentialStoreHandle,
    pub recorder: Arc<dyn HostRuntimeRecorder>,
}

impl SdkHostBindings {
    pub fn permits_credential(&self, scope: &str, provider_id: &str) -> bool {
        provider_id == self.provider_id && self.credential_scopes.contains(scope)
    }
}

#[cfg(test)]
mod bundle_tests;
#[cfg(test)]
mod tests;
