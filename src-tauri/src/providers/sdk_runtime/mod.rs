use std::collections::HashSet;
use std::sync::Arc;

use serde_json::Value;

pub const QUICKJS_RUNTIME_BOOTSTRAP: &str =
    include_str!(concat!(env!("OUT_DIR"), "/sayit-sdk-runtime-bootstrap.js"));

pub trait HostCredentialReader: Send + Sync {
    fn get(&self, scope: &str, provider_id: &str) -> Result<Option<String>, String>;
}

pub trait HostRuntimeRecorder: Send + Sync {
    fn record(&self, event: Value);
}

#[derive(Clone)]
pub struct SdkHostBindings {
    pub owner_id: String,
    pub provider_id: String,
    pub request_id: String,
    pub credential_scopes: HashSet<String>,
    pub credentials: Arc<dyn HostCredentialReader>,
    pub recorder: Arc<dyn HostRuntimeRecorder>,
}

impl SdkHostBindings {
    pub fn permits_credential(&self, scope: &str, provider_id: &str) -> bool {
        provider_id == self.provider_id && self.credential_scopes.contains(scope)
    }
}

#[cfg(test)]
mod tests;
