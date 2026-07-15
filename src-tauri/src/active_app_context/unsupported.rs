use super::{
    ActivationTarget, ActiveAppContextProvider, CaptureOptions, CaptureStatus,
    CapturedActiveAppContext,
};

pub(crate) struct UnsupportedActiveAppContextProvider;

impl ActiveAppContextProvider for UnsupportedActiveAppContextProvider {
    fn capture(
        &self,
        _target: ActivationTarget,
        _blocked_apps: &[String],
        _options: CaptureOptions,
        _cancelled: &std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> CapturedActiveAppContext {
        CapturedActiveAppContext::with_status(CaptureStatus::Unsupported)
    }
}

pub(crate) fn activation_target() -> Option<ActivationTarget> {
    None
}
