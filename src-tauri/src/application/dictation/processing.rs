use std::future::Future;
use tokio_util::sync::CancellationToken;

/// 只包围可取消的上下文与模型等待；原文落盘、资源清理和注入提交仍由外层完成。
pub(super) async fn until_cancelled<T>(
    cancellation: &CancellationToken,
    operation: impl Future<Output = T>,
) -> Option<T> {
    tokio::select! {
        biased;
        _ = cancellation.cancelled() => None,
        result = operation => Some(result),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cancellation::CancellationFlag;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    #[tokio::test]
    async fn already_cancelled_session_does_not_start_processing() {
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        let calls = AtomicUsize::new(0);
        assert!(until_cancelled(&cancellation, async {
            calls.fetch_add(1, Ordering::Relaxed);
        })
        .await
        .is_none());
        assert_eq!(calls.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn processing_result_and_failure_are_preserved() {
        let cancellation = CancellationToken::new();
        assert_eq!(
            until_cancelled(&cancellation, async { Ok::<_, String>("complete") }).await,
            Some(Ok("complete"))
        );
        assert_eq!(
            until_cancelled(&cancellation, async { Err::<String, _>("failed") }).await,
            Some(Err("failed"))
        );
    }

    #[tokio::test]
    async fn cancellation_releases_request_and_unblocks_the_next_event() {
        let cancellation = CancellationToken::new();
        let flag = Arc::new(CancellationFlag::default());
        let pending_flag = flag.clone();
        let pending_cancel = cancellation.clone();
        let (started, ready) = tokio::sync::oneshot::channel();
        let (sender, mut events) = tokio::sync::mpsc::channel(1);
        let consumer = tokio::spawn(async move {
            let completed = until_cancelled(&pending_cancel, async {
                let _guard = pending_flag.cancel_on_drop();
                started.send(()).unwrap();
                std::future::pending::<()>().await;
            })
            .await;
            assert!(completed.is_none());
            events.recv().await
        });
        ready.await.unwrap();
        sender.send("next-session-event").await.unwrap();
        cancellation.cancel();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), consumer)
                .await
                .unwrap()
                .unwrap(),
            Some("next-session-event")
        );
        assert!(
            flag.load(Ordering::Acquire),
            "放弃等待也必须取消 JS 工作线程"
        );
    }

    #[test]
    fn failing_session_cancels_old_work_without_cancelling_the_next_session() {
        let mut session = super::super::Session::default();
        let old = session.processing_cancellation.clone();
        session.mark_failed("failed".into(), None);
        assert!(old.is_cancelled());
        let next = super::super::Session::default();
        assert!(!next.processing_cancellation.is_cancelled());
    }
}
