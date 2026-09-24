use super::super::wait_for_stop;
use super::*;
use crate::cancellation::CancellationFlag;
use std::future::Future;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

#[test]
fn shorter_deadline_wakes_all_waiters_and_extension_never_expires_early() {
    tauri::async_runtime::block_on(async {
        let deadline = Arc::new(Deadline::new(Instant::now() + Duration::from_millis(30)));
        let mut tasks = Vec::new();
        for _ in 0..4 {
            let deadline = deadline.clone();
            tasks.push(tauri::async_runtime::spawn(async move {
                deadline.elapsed().await
            }));
        }
        tokio::task::yield_now().await;
        deadline
            .set(Instant::now() + Duration::from_secs(2))
            .unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(80), &mut tasks[0])
                .await
                .is_err()
        );
        deadline
            .set(Instant::now() + Duration::from_millis(10))
            .unwrap();
        for task in tasks {
            tokio::time::timeout(Duration::from_millis(500), task)
                .await
                .unwrap()
                .unwrap();
        }
    });
}

#[test]
fn repeated_extension_does_not_poll_waiters() {
    tauri::async_runtime::block_on(async {
        let deadline = Arc::new(Deadline::new(Instant::now() + Duration::from_secs(10)));
        let polls = Arc::new(AtomicUsize::new(0));
        let watched = deadline.clone();
        let count = polls.clone();
        let (ready, waiting) = tokio::sync::oneshot::channel();
        let task = tauri::async_runtime::spawn(async move {
            let mut ready = Some(ready);
            let mut future = std::pin::pin!(watched.elapsed());
            std::future::poll_fn(|context| {
                count.fetch_add(1, Ordering::Relaxed);
                let result = future.as_mut().poll(context);
                if let Some(ready) = ready.take() {
                    let _ = ready.send(());
                }
                result
            })
            .await;
        });
        waiting.await.unwrap();
        for _ in 0..100 {
            deadline
                .set(Instant::now() + Duration::from_secs(20))
                .unwrap();
        }
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert_eq!(polls.load(Ordering::Relaxed), 1);
        deadline.set(Instant::now()).unwrap();
        tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .unwrap()
            .unwrap();
    });
}

#[test]
fn cancellation_interrupts_all_network_waiters_independently_of_the_deadline() {
    tauri::async_runtime::block_on(async {
        let cancelled = Arc::new(CancellationFlag::default());
        let deadline = Arc::new(Deadline::new(Instant::now() + Duration::from_secs(60)));
        let mut tasks = Vec::new();
        for _ in 0..8 {
            tasks.push(tauri::async_runtime::spawn(wait_for_stop(
                cancelled.clone(),
                deadline.clone(),
            )));
        }
        cancelled.store(true, Ordering::Release);
        for task in tasks {
            tokio::time::timeout(Duration::from_secs(1), task)
                .await
                .unwrap()
                .unwrap();
        }
        assert!(!deadline.expired());
    });
}
