use serde_json::Value;

#[derive(Clone, Debug)]
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

pub(crate) struct BackendEventHub {
    sender: tokio::sync::broadcast::Sender<BackendEvent>,
}

impl Default for BackendEventHub {
    fn default() -> Self {
        let (sender, _) = tokio::sync::broadcast::channel(256);
        Self { sender }
    }
}

impl BackendEventHub {
    pub(crate) fn subscribe(&self) -> tokio::sync::broadcast::Receiver<BackendEvent> {
        self.sender.subscribe()
    }

    pub(crate) fn publish(&self, event: BackendEvent) {
        let _ = self.sender.send(event);
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
        match receiver.recv().await.unwrap() {
            BackendEvent::Asr { session_id, kind, .. } => {
                assert_eq!(session_id, "s1");
                assert_eq!(kind, "finish");
            }
            _ => panic!("unexpected event"),
        }
    }
}
