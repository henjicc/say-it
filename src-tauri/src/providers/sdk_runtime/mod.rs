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
pub const AI_SDK_LLM_BUNDLE: &str = include_str!(concat!(
    env!("OUT_DIR"),
    "/sayit-ai-sdk-runtime/sayit-ai-sdk-llm.js"
));
pub const AI_SDK_LLM_MODULES_BUNDLE: &str = include_str!(concat!(
    env!("OUT_DIR"),
    "/sayit-ai-sdk-runtime/sayit-ai-sdk-llm-modules.js"
));
pub const AI_SDK_BOOTSTRAP: &str = include_str!(concat!(
    env!("OUT_DIR"),
    "/sayit-ai-sdk-runtime/sayit-ai-sdk-bootstrap.js"
));
pub const AI_SDK_BUNDLE_MANIFEST: &str = include_str!(concat!(
    env!("OUT_DIR"),
    "/sayit-ai-sdk-runtime/sayit-ai-sdk-manifest.json"
));

/// 构建时从 SDK 导出的官方按量标准价：`capabilities` 按能力模块 ID，`llm` 按供应商预设 ID 再按模型 ID。
pub static SDK_PRICING: std::sync::LazyLock<Value> = std::sync::LazyLock::new(|| {
    let mut manifest: Value =
        serde_json::from_str(AI_SDK_BUNDLE_MANIFEST).expect("SDK 清单是构建脚本生成的 JSON");
    manifest["pricing"].take()
});

/// 构建时按 SDK 地址解析算出的、内置大模型会访问的主机，按 SDK 供应商 ID。
pub static SDK_LLM_HOSTS: std::sync::LazyLock<std::collections::HashMap<String, Vec<String>>> =
    std::sync::LazyLock::new(|| {
        let mut manifest: Value =
            serde_json::from_str(AI_SDK_BUNDLE_MANIFEST).expect("SDK 清单是构建脚本生成的 JSON");
        serde_json::from_value(manifest["llmHosts"].take())
            .expect("SDK 清单的 llmHosts 是按供应商的主机列表")
    });

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
mod live_result;
#[cfg(test)]
mod live_tests;
#[cfg(test)]
mod tests;
