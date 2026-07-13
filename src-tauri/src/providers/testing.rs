//! 无网络能力桩。只模拟标准化业务事件，不复制任何供应商协议字段。
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FakeEvent {
    Submitted,
    Progress(u8),
    Delta(String),
    Completed(String),
    Failed(String),
}

pub struct FakeProvider {
    events: Vec<FakeEvent>,
    cancelled: Arc<AtomicBool>,
}
impl FakeProvider {
    pub fn new(events: Vec<FakeEvent>) -> Self {
        Self {
            events,
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }
    pub fn cancel_flag(&self) -> Arc<AtomicBool> {
        self.cancelled.clone()
    }
    pub fn run(&self) -> Vec<FakeEvent> {
        self.events
            .iter()
            .take_while(|_| !self.cancelled.load(Ordering::Relaxed))
            .cloned()
            .collect()
    }
}

#[test]
fn file_success_and_async_progress_are_protocol_neutral() {
    let provider = FakeProvider::new(vec![
        FakeEvent::Submitted,
        FakeEvent::Progress(50),
        FakeEvent::Completed("结果".into()),
    ]);
    assert_eq!(
        provider.run(),
        vec![
            FakeEvent::Submitted,
            FakeEvent::Progress(50),
            FakeEvent::Completed("结果".into())
        ]
    );
}

#[test]
fn cancellation_suppresses_late_completion() {
    let provider = FakeProvider::new(vec![
        FakeEvent::Submitted,
        FakeEvent::Completed("迟到结果".into()),
    ]);
    provider.cancel_flag().store(true, Ordering::Relaxed);
    assert!(provider.run().is_empty());
}

#[test]
fn translation_delta_and_error_remain_distinct() {
    let provider = FakeProvider::new(vec![
        FakeEvent::Delta("你".into()),
        FakeEvent::Failed("流中断".into()),
    ]);
    assert_eq!(
        provider.run(),
        vec![
            FakeEvent::Delta("你".into()),
            FakeEvent::Failed("流中断".into())
        ]
    );
}
