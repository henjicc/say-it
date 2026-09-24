use super::*;
use std::sync::Arc;
use std::time::Duration;

#[test]
fn cancellation_before_subscription_and_racing_registration_is_persistent() {
    tauri::async_runtime::block_on(async {
        for _ in 0..128 {
            let flag = Arc::new(CancellationFlag::default());
            let waiting = flag.clone();
            let task = tauri::async_runtime::spawn(async move { waiting.cancelled().await });
            flag.store(true, Ordering::Release);
            tokio::time::timeout(Duration::from_secs(1), task)
                .await
                .unwrap()
                .unwrap();
            flag.cancelled().await;
        }
    });
}

#[test]
fn one_cancel_wakes_every_waiter_and_repeated_cancel_keeps_atomic_semantics() {
    tauri::async_runtime::block_on(async {
        let flag = Arc::new(CancellationFlag::default());
        let mut tasks = Vec::new();
        for _ in 0..16 {
            let flag = flag.clone();
            tasks.push(tauri::async_runtime::spawn(async move {
                flag.cancelled().await
            }));
        }
        tokio::task::yield_now().await;
        assert!(!flag.swap(true, Ordering::AcqRel));
        assert!(flag.swap(true, Ordering::AcqRel));
        for task in tasks {
            tokio::time::timeout(Duration::from_secs(1), task)
                .await
                .unwrap()
                .unwrap();
        }
    });
}

#[test]
fn false_notification_does_not_complete_cancellation() {
    tauri::async_runtime::block_on(async {
        let flag = CancellationFlag::default();
        flag.store(false, Ordering::Release);
        assert!(
            tokio::time::timeout(Duration::from_millis(20), flag.cancelled())
                .await
                .is_err()
        );
        flag.store(true, Ordering::Release);
        tokio::time::timeout(Duration::from_secs(1), flag.cancelled())
            .await
            .unwrap();
    });
}
