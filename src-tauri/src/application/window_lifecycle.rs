#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum MainWindowPhase {
    #[default]
    Absent,
    Creating,
    Ready,
    Visible,
    Closing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EnsureMainWindowAction {
    Create { generation: u64 },
    AwaitReady,
    ShowExisting,
}

/// 主窗口生命周期只管理 WebView，不拥有任何听写、字幕或其他业务会话。
/// `generation` 用于阻止较早的创建失败覆盖较新的创建状态。
#[derive(Debug, Default)]
pub(crate) struct MainWindowLifecycle {
    phase: MainWindowPhase,
    generation: u64,
    open_requested: bool,
}

impl MainWindowLifecycle {
    #[cfg(test)]
    fn phase(&self) -> MainWindowPhase {
        self.phase
    }

    pub(crate) fn register_initial_window(&mut self, should_open: bool) {
        self.phase = MainWindowPhase::Creating;
        self.open_requested = should_open;
        self.generation = self.generation.wrapping_add(1);
    }

    pub(crate) fn request_open(&mut self, window_exists: bool) -> EnsureMainWindowAction {
        self.open_requested = true;
        if !window_exists {
            self.phase = MainWindowPhase::Creating;
            self.generation = self.generation.wrapping_add(1);
            return EnsureMainWindowAction::Create {
                generation: self.generation,
            };
        }

        match self.phase {
            MainWindowPhase::Creating | MainWindowPhase::Closing => {
                EnsureMainWindowAction::AwaitReady
            }
            MainWindowPhase::Absent | MainWindowPhase::Ready | MainWindowPhase::Visible => {
                self.phase = MainWindowPhase::Visible;
                EnsureMainWindowAction::ShowExisting
            }
        }
    }

    pub(crate) fn creation_failed(&mut self, generation: u64) {
        if self.phase == MainWindowPhase::Creating && self.generation == generation {
            self.phase = MainWindowPhase::Absent;
            self.open_requested = false;
        }
    }

    pub(crate) fn mark_ready(&mut self) -> bool {
        if !matches!(
            self.phase,
            MainWindowPhase::Creating | MainWindowPhase::Ready
        ) {
            return self.phase == MainWindowPhase::Visible;
        }
        let should_show = self.open_requested;
        self.phase = if should_show {
            MainWindowPhase::Visible
        } else {
            MainWindowPhase::Ready
        };
        should_show
    }

    pub(crate) fn begin_close(&mut self) {
        self.phase = MainWindowPhase::Closing;
        self.open_requested = false;
    }

    pub(crate) fn close_completed(&mut self) {
        self.phase = MainWindowPhase::Absent;
        self.open_requested = false;
    }

    pub(crate) fn close_failed_hidden(&mut self) {
        self.phase = MainWindowPhase::Ready;
        self.open_requested = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_open_while_creating_never_requests_a_second_window() {
        let mut lifecycle = MainWindowLifecycle::default();
        assert_eq!(
            lifecycle.request_open(false),
            EnsureMainWindowAction::Create { generation: 1 }
        );
        assert_eq!(
            lifecycle.request_open(true),
            EnsureMainWindowAction::AwaitReady
        );
        assert!(lifecycle.mark_ready());
        assert_eq!(lifecycle.phase(), MainWindowPhase::Visible);
    }

    #[test]
    fn create_failure_is_recoverable_and_stale_failure_is_ignored() {
        let mut lifecycle = MainWindowLifecycle::default();
        assert_eq!(
            lifecycle.request_open(false),
            EnsureMainWindowAction::Create { generation: 1 }
        );
        lifecycle.creation_failed(1);
        assert_eq!(lifecycle.phase(), MainWindowPhase::Absent);
        assert_eq!(
            lifecycle.request_open(false),
            EnsureMainWindowAction::Create { generation: 2 }
        );
        lifecycle.creation_failed(1);
        assert_eq!(lifecycle.phase(), MainWindowPhase::Creating);
    }

    #[test]
    fn closing_does_not_restart_or_own_background_domains() {
        let mut lifecycle = MainWindowLifecycle::default();
        lifecycle.register_initial_window(true);
        assert!(lifecycle.mark_ready());
        lifecycle.begin_close();
        assert_eq!(lifecycle.phase(), MainWindowPhase::Closing);
        lifecycle.close_completed();
        assert_eq!(lifecycle.phase(), MainWindowPhase::Absent);
    }

    #[test]
    fn hidden_initial_window_stays_ready_until_explicit_open() {
        let mut lifecycle = MainWindowLifecycle::default();
        lifecycle.register_initial_window(false);
        assert!(!lifecycle.mark_ready());
        assert_eq!(lifecycle.phase(), MainWindowPhase::Ready);
        assert_eq!(
            lifecycle.request_open(true),
            EnsureMainWindowAction::ShowExisting
        );
        assert_eq!(lifecycle.phase(), MainWindowPhase::Visible);
    }
}
