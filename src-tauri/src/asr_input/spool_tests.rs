use super::*;
use std::time::{Duration, Instant};
fn small_limits() -> Limits {
    Limits {
        memory: 8192,
        ..Default::default()
    }
}
fn wait_clean(handle: &AsrStreamHandle) {
    let until = Instant::now() + Duration::from_secs(3);
    loop {
        if handle.tx.budget.resident.load(Ordering::Acquire) == 0
            && handle.tx.spool.retained_disk_bytes() == 0
            && handle
                .tx
                .spool
                .test_paths()
                .iter()
                .all(|path| !path.exists())
        {
            break;
        }
        assert!(Instant::now() < until, "暂存资源没有退出");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(handle.tx.budget.bytes.load(Ordering::Acquire), 0);
    assert_eq!(handle.tx.budget.packets.load(Ordering::Acquire), 0);
    assert_eq!(handle.tx.spool.retained_disk_bytes(), 0);
}
#[test]
fn short_queue_never_creates_a_file_or_worker() {
    let (handle, mut rx) = AsrStreamHandle::channel();
    for i in 0..500 {
        handle
            .tx
            .send(AsrStreamInput::RawF32(vec![i as f32; 4096]))
            .unwrap();
        let Some(AsrStreamInput::RawF32(samples)) = rx.blocking_recv() else {
            panic!()
        };
        assert_eq!(samples, vec![i as f32; 4096]);
    }
    assert!(handle.tx.spool.test_paths().is_empty());
    drop(rx);
    wait_clean(&handle);
}
#[test]
fn spilled_queue_preserves_order_bits_and_finish_for_all_receive_modes() {
    for mode in 0..3 {
        let (handle, mut rx) = AsrStreamHandle::with_limits(small_limits());
        for i in 0..150 {
            let mut input = vec![i as f32; 4096];
            input[0] = f32::from_bits(0x7fc01234);
            input[1] = -0.0;
            handle.tx.send(AsrStreamInput::RawF32(input)).unwrap();
        }
        handle.tx.send(AsrStreamInput::Finish).unwrap();
        for i in 0..150 {
            let item = match mode {
                0 => rx.blocking_recv(),
                1 => loop {
                    match rx.try_recv() {
                        Ok(item) => break Some(item),
                        Err(mpsc::error::TryRecvError::Empty) => std::thread::yield_now(),
                        Err(_) => panic!("提前退出"),
                    }
                },
                _ => tauri::async_runtime::block_on(rx.recv()),
            };
            let Some(AsrStreamInput::RawF32(samples)) = item else {
                panic!("丢失音频包")
            };
            assert_eq!(samples.len(), 4096);
            assert_eq!(samples[0].to_bits(), 0x7fc01234);
            assert_eq!(samples[1].to_bits(), (-0.0f32).to_bits());
            assert!(samples[2..].iter().all(|sample| *sample == i as f32));
        }
        assert!(matches!(rx.blocking_recv(), Some(AsrStreamInput::Finish)));
        assert!(!handle.tx.spool.test_paths().is_empty());
        drop(rx);
        wait_clean(&handle);
    }
}
#[test]
fn cancelling_spilled_audio_releases_writer_buffers_and_files() {
    for _ in 0..8 {
        let (handle, mut rx) = AsrStreamHandle::with_limits(small_limits());
        for _ in 0..300 {
            handle
                .tx
                .send(AsrStreamInput::RawF32(vec![0.25; 4096]))
                .unwrap();
        }
        handle.stop();
        assert!(matches!(rx.blocking_recv(), Some(AsrStreamInput::Stop)));
        wait_clean(&handle);
    }
}
#[test]
fn each_limit_is_a_visible_failure_before_accepting_more_audio() {
    let base = Limits {
        memory: usize::MAX,
        ..Default::default()
    };
    for limits in [
        Limits { packet: 4, ..base },
        Limits {
            resident: 4,
            ..base
        },
        Limits { queued: 4, ..base },
        Limits { packets: 0, ..base },
    ] {
        let (handle, mut rx) = AsrStreamHandle::with_limits(limits);
        assert!(handle
            .tx
            .send(AsrStreamInput::RawF32(vec![0.25; 4096]))
            .is_err());
        let Some(AsrStreamInput::Failed(error)) = rx.blocking_recv() else {
            panic!("必须报告错误")
        };
        assert!(!error.is_empty());
        wait_clean(&handle);
    }
}
#[test]
fn actual_write_failure_interrupts_queue_and_reports_reason() {
    let (handle, mut rx) = AsrStreamHandle::with_limits(small_limits());
    handle.tx.spool.reject_writes_for_test();
    handle
        .tx
        .send(AsrStreamInput::RawF32(vec![0.25; 4096]))
        .unwrap();
    let Some(AsrStreamInput::Failed(error)) = rx.blocking_recv() else {
        panic!("写入错误不能成为完成")
    };
    assert!(error.contains("写入"), "{error}");
    drop(rx);
    wait_clean(&handle);
}

#[test]
fn actual_read_failure_ends_receiver_and_preserves_error_for_busy_session() {
    let (handle, mut rx) = AsrStreamHandle::with_limits(small_limits());
    handle
        .tx
        .send(AsrStreamInput::RawF32(vec![0.25; 4096]))
        .unwrap();
    let mut queued = rx.inner.as_mut().unwrap().blocking_recv().unwrap();
    let Some(Payload::Disk(ticket)) = queued.payload.take() else {
        panic!("必须进入真实暂存")
    };
    let packet = ticket.blocking_recv().unwrap().unwrap();
    packet.truncate_for_test(4);
    let (reply, ticket) = tokio::sync::oneshot::channel();
    assert!(reply.send(Ok(packet)).is_ok());
    queued.payload = Some(Payload::Disk(ticket));
    rx.pending = Some(queued);
    let Some(AsrStreamInput::Failed(error)) = rx.blocking_recv() else {
        panic!("读盘错误不能成为成功")
    };
    assert!(error.contains("不完整"), "{error}");
    assert_eq!(rx.take_failure(), Some(error));
    assert!(rx.take_failure().is_none());
    assert!(rx.blocking_recv().is_none());
    wait_clean(&handle);
}

#[test]
fn concurrent_live_producers_keep_each_source_order_and_release_all_charges() {
    let (handle, mut rx) = AsrStreamHandle::with_limits(small_limits());
    let workers: Vec<_> = (0..4)
        .map(|source| {
            let tx = handle.tx.clone();
            std::thread::spawn(move || {
                for sequence in 0..300 {
                    let mut samples = vec![0.25; 4096];
                    samples[0] = source as f32;
                    samples[1] = sequence as f32;
                    tx.send(AsrStreamInput::RawF32(samples)).unwrap();
                }
            })
        })
        .collect();
    for worker in workers {
        worker.join().unwrap();
    }
    let mut counts = [0; 4];
    for _ in 0..1200 {
        let Some(AsrStreamInput::RawF32(samples)) = rx.blocking_recv() else {
            panic!()
        };
        let source = samples[0] as usize;
        assert_eq!(samples[1] as usize, counts[source]);
        counts[source] += 1;
    }
    assert_eq!(counts, [300; 4]);
    drop(rx);
    wait_clean(&handle);
}
#[test]
fn dropping_async_receive_keeps_dequeued_disk_and_read_tickets() {
    tauri::async_runtime::block_on(async {
        for reading in [false, true] {
            let (handle, mut rx) = AsrStreamHandle::channel();
            let charge = QueueCharge {
                budget: handle.tx.budget.clone(),
                bytes: 1,
            };
            handle.tx.budget.bytes.store(1, Ordering::Release);
            handle.tx.budget.packets.store(1, Ordering::Release);
            let (disk_sender, disk_ticket) = tokio::sync::oneshot::channel();
            let (read_sender, read_ticket) = tokio::sync::oneshot::channel();
            let payload = if reading {
                Payload::Reading(read_ticket)
            } else {
                Payload::Disk(disk_ticket)
            };
            assert!(handle
                .tx
                .inner
                .send(QueuedInput {
                    payload: Some(payload),
                    _charge: charge,
                    _resident: None
                })
                .is_ok());
            let mut receive = Box::pin(rx.recv());
            assert!(futures_util::poll!(receive.as_mut()).is_pending());
            drop(receive);
            assert!(rx.pending.is_some());
            assert_eq!(handle.tx.budget.bytes.load(Ordering::Acquire), 1);
            if reading {
                assert!(read_sender
                    .send((spool::Reader::default(), Ok(vec![0.25; 17])))
                    .is_ok());
                let Some(AsrStreamInput::RawF32(samples)) = rx.recv().await else {
                    panic!()
                };
                assert_eq!(samples, vec![0.25; 17]);
            } else {
                handle.stop();
                assert!(matches!(rx.recv().await, Some(AsrStreamInput::Stop)));
                drop(disk_sender);
            }
            assert_eq!(handle.tx.budget.bytes.load(Ordering::Acquire), 0);
        }
    });
}
