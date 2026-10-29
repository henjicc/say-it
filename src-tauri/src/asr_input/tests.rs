use super::*;
use std::time::Duration;

#[test]
fn finish_preserves_audio_order_and_all_tail_samples() {
    let (handle, mut rx) = AsrStreamHandle::channel();
    for i in 0..100 {
        handle
            .tx
            .send(AsrStreamInput::RawF32(vec![i as f32; i + 1]))
            .unwrap();
    }
    handle.tx.send(AsrStreamInput::Finish).unwrap();
    for i in 0..100 {
        let Some(AsrStreamInput::RawF32(samples)) = rx.blocking_recv() else {
            panic!("audio order")
        };
        assert_eq!(samples, vec![i as f32; i + 1]);
    }
    assert!(matches!(rx.blocking_recv(), Some(AsrStreamInput::Finish)));
    assert!(!rx.is_cancelled());
}

#[test]
fn cancellation_overtakes_backlog_releases_it_and_closes_old_senders() {
    for blocking in [true, false] {
        let (handle, mut rx) = AsrStreamHandle::channel();
        let old_sender = handle.tx.clone();
        for _ in 0..1000 {
            handle
                .tx
                .send(AsrStreamInput::RawF32(vec![0.125; 4096]))
                .unwrap();
        }
        handle.tx.send(AsrStreamInput::Finish).unwrap();
        let cancellation = rx.cancellation_flag();
        handle.stop();
        assert!(cancellation.load(Ordering::Acquire));
        let input = if blocking {
            rx.blocking_recv()
        } else {
            rx.try_recv().ok()
        };
        assert!(matches!(input, Some(AsrStreamInput::Stop)));
        assert!(rx.inner.is_none(), "积压必须释放");
        assert!(old_sender.send(AsrStreamInput::RawF32(vec![1.0])).is_err());
        assert!(matches!(
            rx.try_recv(),
            Err(mpsc::error::TryRecvError::Disconnected)
        ));
        handle.stop(); // 重复取消无副作用。
    }
}

#[test]
fn stop_wakes_a_blocked_receiver_and_does_not_cancel_another_session() {
    let (handle, mut rx) = AsrStreamHandle::channel();
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        ready_tx.send(()).unwrap();
        done_tx
            .send(matches!(rx.blocking_recv(), Some(AsrStreamInput::Stop)))
            .unwrap();
    });
    ready_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let (other, mut other_rx) = AsrStreamHandle::channel();
    handle.stop();
    assert!(done_rx.recv_timeout(Duration::from_secs(2)).unwrap());
    worker.join().unwrap();
    assert!(!other_rx.is_cancelled());
    other.tx.send(AsrStreamInput::Finish).unwrap();
    assert!(matches!(other_rx.try_recv(), Ok(AsrStreamInput::Finish)));
}

#[test]
fn async_receive_and_blocked_writer_can_be_cancelled_without_lost_wakeup() {
    tauri::async_runtime::block_on(async {
        use tokio::io::AsyncWriteExt;
        for pre_cancel in [true, false] {
            let (handle, mut rx) = AsrStreamHandle::channel();
            if pre_cancel {
                handle.stop();
            }
            let mut receive = Box::pin(rx.recv());
            if !pre_cancel {
                assert!(futures_util::poll!(receive.as_mut()).is_pending());
                handle.stop();
            }
            assert!(matches!(
                tokio::time::timeout(Duration::from_secs(2), receive)
                    .await
                    .unwrap(),
                Some(AsrStreamInput::Stop)
            ));
        }
        let (handle, rx) = AsrStreamHandle::channel();
        let cancellation = rx.cancellation();
        let (mut writer, _unread) = tokio::io::duplex(1);
        let payload = [1; 4096];
        let mut write = Box::pin(writer.write_all(&payload));
        assert!(futures_util::poll!(write.as_mut()).is_pending());
        let mut cancelled = Box::pin(cancellation.cancelled());
        assert!(futures_util::poll!(cancelled.as_mut()).is_pending());
        handle.stop();
        let stopped = tokio::time::timeout(Duration::from_secs(2), async {
            tokio::select! {
                biased;
                _ = cancelled => true,
                _ = write => false,
            }
        })
        .await
        .unwrap();
        assert!(stopped);
    });
}

#[test]
fn public_stop_command_uses_priority_signal_and_finish_does_not() {
    let state = crate::state::RuntimeState::default();
    let (handle, mut rx) = AsrStreamHandle::channel();
    handle
        .tx
        .send(AsrStreamInput::RawF32(vec![1.0; 4096]))
        .unwrap();
    state
        .asr_streams
        .lock()
        .unwrap()
        .insert("test".into(), handle);
    crate::commands::asr::asr_stream_finish_inner("test", &state).unwrap();
    assert!(!rx.is_cancelled());
    crate::commands::asr::stop_asr_stream_inner("test", &state).unwrap();
    assert!(matches!(rx.blocking_recv(), Some(AsrStreamInput::Stop)));
    assert!(state.asr_streams.lock().unwrap().is_empty());
}
