use super::*;
use std::time::Duration;

fn fill(handle: &AsrStreamHandle) {
    let packet = vec![0.25; 1600];
    let cost = QueuedInput::cost(&AsrStreamInput::RawF32(packet.clone()));
    for _ in 0..PACED_QUEUE_BYTES / cost {
        handle
            .tx
            .send(AsrStreamInput::RawF32(packet.clone()))
            .unwrap();
    }
}

#[test]
fn waiting_producer_wakes_on_space_and_finish_follows_every_tail() {
    tauri::async_runtime::block_on(async {
        let (handle, mut rx) = AsrStreamHandle::channel();
        fill(&handle);
        let before = handle.tx.budget.bytes.load(Ordering::Acquire);
        let mut pending = Box::pin(
            handle
                .tx
                .send_paced(AsrStreamInput::RawF32(vec![0.5; 1600])),
        );
        assert!(futures_util::poll!(pending.as_mut()).is_pending());
        assert_eq!(handle.tx.budget.bytes.load(Ordering::Acquire), before);
        assert!(matches!(rx.try_recv(), Ok(AsrStreamInput::RawF32(_))));
        tokio::time::timeout(Duration::from_secs(2), pending)
            .await
            .unwrap()
            .unwrap();
        while matches!(rx.try_recv(), Ok(AsrStreamInput::RawF32(_))) {}
        assert_eq!(handle.tx.budget.bytes.load(Ordering::Acquire), 0);
        handle
            .tx
            .send_paced(AsrStreamInput::RawF32(vec![0.75; 17]))
            .await
            .unwrap();
        handle.tx.send(AsrStreamInput::Finish).unwrap();
        let Some(AsrStreamInput::RawF32(tail)) = rx.recv().await else {
            panic!("missing tail")
        };
        assert_eq!(tail, vec![0.75; 17]);
        assert!(matches!(rx.recv().await, Some(AsrStreamInput::Finish)));
        assert_eq!(handle.tx.budget.bytes.load(Ordering::Acquire), 0);
    });
}

#[test]
fn cancellation_wakes_all_producers_without_waiting_for_recognizer() {
    tauri::async_runtime::block_on(async {
        let (handle, mut rx) = AsrStreamHandle::channel();
        fill(&handle);
        let mut a = Box::pin(
            handle
                .tx
                .send_paced(AsrStreamInput::RawF32(vec![1.0; 1600])),
        );
        let mut b = Box::pin(
            handle
                .tx
                .send_paced(AsrStreamInput::RawF32(vec![2.0; 1600])),
        );
        assert!(futures_util::poll!(a.as_mut()).is_pending());
        assert!(futures_util::poll!(b.as_mut()).is_pending());
        handle.stop();
        assert!(tokio::time::timeout(Duration::from_secs(2), a)
            .await
            .unwrap()
            .is_err());
        assert!(tokio::time::timeout(Duration::from_secs(2), b)
            .await
            .unwrap()
            .is_err());
        assert!(matches!(rx.try_recv(), Ok(AsrStreamInput::Stop)));
        assert_eq!(handle.tx.budget.bytes.load(Ordering::Acquire), 0);
    });
}

#[test]
fn receiver_failure_and_abandoned_send_return_all_budget() {
    tauri::async_runtime::block_on(async {
        let (handle, rx) = AsrStreamHandle::channel();
        fill(&handle);
        let before = handle.tx.budget.bytes.load(Ordering::Acquire);
        let mut pending = Box::pin(
            handle
                .tx
                .send_paced(AsrStreamInput::RawF32(vec![1.0; 1600])),
        );
        assert!(futures_util::poll!(pending.as_mut()).is_pending());
        drop(pending);
        assert_eq!(handle.tx.budget.bytes.load(Ordering::Acquire), before);
        let mut pending = Box::pin(
            handle
                .tx
                .send_paced(AsrStreamInput::RawF32(vec![2.0; 1600])),
        );
        assert!(futures_util::poll!(pending.as_mut()).is_pending());
        drop(rx);
        assert!(tokio::time::timeout(Duration::from_secs(2), pending)
            .await
            .unwrap()
            .is_err());
        assert_eq!(handle.tx.budget.bytes.load(Ordering::Acquire), 0);
        assert!(handle
            .tx
            .send(AsrStreamInput::RawF32(vec![3.0; 1600]))
            .is_err());
        assert_eq!(handle.tx.budget.bytes.load(Ordering::Acquire), 0);
    });
}

#[test]
fn budget_counts_allocation_capacity_and_never_waits_for_oversized_packet() {
    tauri::async_runtime::block_on(async {
        let (handle, _rx) = AsrStreamHandle::channel();
        let mut input = Vec::with_capacity(PACED_QUEUE_BYTES);
        input.push(0.5);
        let result = tokio::time::timeout(
            Duration::from_secs(2),
            handle.tx.send_paced(AsrStreamInput::RawF32(input)),
        )
        .await
        .unwrap();
        assert!(result.is_err());
        assert_eq!(handle.tx.budget.bytes.load(Ordering::Acquire), 0);
    });
}

#[test]
fn concurrent_producers_share_one_budget_and_deliver_all_samples_in_order() {
    tauri::async_runtime::block_on(async {
        let (handle, mut rx) = AsrStreamHandle::channel();
        let budget = handle.tx.budget.clone();
        let mut workers = Vec::new();
        for id in 0..4 {
            let tx = handle.tx.clone();
            workers.push(tauri::async_runtime::spawn(async move {
                for sequence in 0..200 {
                    let mut packet = vec![id as f32; 1600];
                    packet[1] = sequence as f32;
                    tx.send_paced(AsrStreamInput::RawF32(packet)).await.unwrap();
                }
            }));
        }
        drop(handle);
        let mut counts = [0; 4];
        while let Some(packet) = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .unwrap()
        {
            assert!(budget.bytes.load(Ordering::Acquire) <= PACED_QUEUE_BYTES);
            let AsrStreamInput::RawF32(samples) = packet else {
                panic!()
            };
            let id = samples[0] as usize;
            assert_eq!(samples[1] as usize, counts[id]);
            assert_eq!(samples.len(), 1600);
            counts[id] += 1;
            tokio::task::yield_now().await;
        }
        for worker in workers {
            worker.await.unwrap();
        }
        assert_eq!(counts, [200; 4]);
        assert_eq!(budget.bytes.load(Ordering::Acquire), 0);
    });
}

#[test]
fn paced_input_accounts_for_existing_nonwaiting_backlog() {
    tauri::async_runtime::block_on(async {
        let (handle, mut rx) = AsrStreamHandle::channel();
        for _ in 0..20 {
            handle
                .tx
                .send(AsrStreamInput::RawF32(vec![0.25; 1600]))
                .unwrap();
        }
        let mut pending = Box::pin(
            handle
                .tx
                .send_paced(AsrStreamInput::RawF32(vec![0.5; 1600])),
        );
        assert!(futures_util::poll!(pending.as_mut()).is_pending());
        for _ in 0..5 {
            rx.try_recv().unwrap();
        }
        assert!(futures_util::poll!(pending.as_mut()).is_pending());
        while rx.try_recv().is_ok() {}
        tokio::time::timeout(Duration::from_secs(2), pending)
            .await
            .unwrap()
            .unwrap();
        assert!(handle.tx.budget.bytes.load(Ordering::Acquire) <= PACED_QUEUE_BYTES);
    });
}
