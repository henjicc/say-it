use serde_json::Value;
use std::sync::Arc;

#[derive(Debug)]
#[cfg_attr(test, derive(Clone))]
pub(crate) enum BackendEvent {
    Asr {
        session_id: String,
        kind: String,
        payload: Value,
    },
    Transcription {
        job_id: String,
        stage: String,
        payload: Value,
    },
}

#[derive(Clone)]
pub(crate) struct BackendEventHub {
    sender: tokio::sync::broadcast::Sender<Arc<BackendEvent>>,
}

impl Default for BackendEventHub {
    fn default() -> Self {
        let (sender, _) = tokio::sync::broadcast::channel(256);
        Self { sender }
    }
}

impl BackendEventHub {
    pub(crate) fn subscribe(&self) -> tokio::sync::broadcast::Receiver<Arc<BackendEvent>> {
        self.sender.subscribe()
    }

    pub(crate) fn publish(&self, event: BackendEvent) {
        // 所有订阅者只读同一份事件；载荷随最后一个接收者释放，队列容量保持不变。
        let _ = self.sender.send(Arc::new(event));
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn publishes_to_rust_subscribers_without_webview() {
        let hub = BackendEventHub::default();
        let mut receiver = hub.subscribe();
        hub.publish(BackendEvent::Asr {
            session_id: "s1".into(),
            kind: "finish".into(),
            payload: serde_json::json!({}),
        });
        match receiver.recv().await.unwrap().as_ref() {
            BackendEvent::Asr {
                session_id, kind, ..
            } => {
                assert_eq!(session_id, "s1");
                assert_eq!(kind, "finish");
            }
            _ => panic!("unexpected event"),
        }
    }

    #[test]
    fn subscribers_share_complete_payload_and_release_it_after_last_use() {
        let hub = BackendEventHub::default();
        let mut first = hub.subscribe();
        let mut second = hub.subscribe();
        let mut third = hub.subscribe();
        let payload = serde_json::json!({"text":"完整文本", "words":[{"beginTime":12,"endTime":34,"text":"完整"}]});
        hub.publish(BackendEvent::Transcription {
            job_id: "job".into(),
            stage: "completed".into(),
            payload: payload.clone(),
        });
        let a = first.try_recv().unwrap();
        let b = second.try_recv().unwrap();
        let c = third.try_recv().unwrap();
        assert!(Arc::ptr_eq(&a, &b) && Arc::ptr_eq(&a, &c));
        match a.as_ref() {
            BackendEvent::Transcription {
                payload: actual, ..
            } => assert_eq!(actual, &payload),
            _ => panic!("wrong event"),
        }
        let weak = Arc::downgrade(&a);
        drop((a, b));
        assert!(weak.upgrade().is_some());
        drop(c);
        assert!(
            weak.upgrade().is_none(),
            "通道不能在全部消费之后永久保留载荷"
        );
    }

    #[test]
    fn dropping_a_slow_subscriber_releases_its_pending_payload() {
        let hub = BackendEventHub::default();
        let mut fast = hub.subscribe();
        let slow = hub.subscribe();
        hub.publish(BackendEvent::Asr {
            session_id: "s".into(),
            kind: "result".into(),
            payload: serde_json::json!({"text":"译文", "final":true}),
        });
        let event = fast.try_recv().unwrap();
        let weak = Arc::downgrade(&event);
        drop(event);
        assert!(weak.upgrade().is_some());
        drop(slow);
        assert!(weak.upgrade().is_none());
    }

    #[test]
    fn lag_remains_explicit_and_overwritten_payload_is_released() {
        let hub = BackendEventHub::default();
        let publisher = hub.clone();
        let mut fast = hub.subscribe();
        let mut slow = hub.subscribe();
        let mut first = None;
        for index in 0..260 {
            publisher.publish(BackendEvent::Asr {
                session_id: "s".into(),
                kind: "result".into(),
                payload: serde_json::json!({"sequence":index}),
            });
            let event = fast.try_recv().unwrap();
            if index == 0 {
                first = Some(Arc::downgrade(&event));
            }
            match event.as_ref() {
                BackendEvent::Asr { payload, .. } => assert_eq!(payload["sequence"], index),
                _ => panic!("wrong event"),
            }
        }
        assert!(first.unwrap().upgrade().is_none());
        assert!(matches!(
            slow.try_recv(),
            Err(tokio::sync::broadcast::error::TryRecvError::Lagged(4))
        ));
        match slow.try_recv().unwrap().as_ref() {
            BackendEvent::Asr { payload, .. } => assert_eq!(payload["sequence"], 4),
            _ => panic!("wrong event"),
        }
    }
}

#[cfg(all(test, windows))]
mod performance_tests;
