use super::{
    ActivationTarget, ActiveAppContextProvider, AppIdentity, CaptureOptions, CaptureStatus,
    CapturedActiveAppContext,
};

pub(crate) struct UnsupportedActiveAppContextProvider;

impl ActiveAppContextProvider for UnsupportedActiveAppContextProvider {
    fn capture(
        &self,
        _target: ActivationTarget,
        _blocked_apps: &[String],
        options: CaptureOptions,
        _cancelled: &std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> CapturedActiveAppContext {
        CapturedActiveAppContext {
            status: CaptureStatus::Unsupported,
            capture_method: options.method,
            ..Default::default()
        }
    }
}

pub(crate) fn activation_target() -> Option<ActivationTarget> {
    None
}

pub(crate) fn app_identity(_target: ActivationTarget) -> Option<AppIdentity> {
    None
}

pub(crate) fn list_running_apps() -> Vec<AppIdentity> {
    Vec::new()
}
