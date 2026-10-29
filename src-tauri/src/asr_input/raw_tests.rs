use super::*;

#[test]
fn closing_capture_drains_spilled_audio_and_partial_tail_in_order() {
    for asynchronous in [false, true] {
        let (handle, receiver) = AsrStreamHandle::with_limits(Limits {
            memory: 8192,
            ..Default::default()
        });
        let mut receiver = RawAudioReceiver(receiver);
        for i in 0..200 {
            handle
                .tx
                .send(AsrStreamInput::RawF32(vec![i as f32; 4096]))
                .unwrap();
        }
        handle
            .tx
            .send(AsrStreamInput::RawF32(vec![-0.0; 17]))
            .unwrap();
        let spool = handle.tx.spool.clone();
        drop(handle);
        for i in 0..201 {
            let samples = if asynchronous {
                tauri::async_runtime::block_on(receiver.recv())
            } else {
                receiver.blocking_recv()
            }
            .unwrap()
            .expect("正常关闭必须先排空");
            if i == 200 {
                assert_eq!(samples.len(), 17);
                assert!(samples
                    .iter()
                    .all(|sample| sample.to_bits() == (-0.0f32).to_bits()));
            } else {
                assert_eq!(samples, vec![i as f32; 4096]);
            }
        }
        assert!(receiver.blocking_recv().unwrap().is_none());
        assert!(!spool.test_paths().is_empty());
        drop(receiver);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while spool.retained_disk_bytes() != 0 {
            assert!(std::time::Instant::now() < deadline, "正常关闭未回收暂存");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(spool.retained_disk_bytes(), 0);
    }
}

#[test]
fn capture_failure_is_an_error_even_when_audio_is_waiting() {
    for asynchronous in [false, true] {
        let (sender, mut receiver) = RawAudioReceiver::channel();
        sender
            .send(AsrStreamInput::RawF32(vec![0.25; 4096]))
            .unwrap();
        sender.fail("输入设备已断开".into());
        assert!(sender.is_closed());
        let result = if asynchronous {
            tauri::async_runtime::block_on(receiver.recv())
        } else {
            receiver.blocking_recv()
        };
        assert_eq!(result.unwrap_err(), "输入设备已断开");
    }
}

#[test]
fn failed_spool_cannot_be_mistaken_for_normal_capture_end() {
    let (handle, receiver) = AsrStreamHandle::with_limits(Limits {
        memory: 0,
        ..Default::default()
    });
    handle.tx.spool.reject_writes_for_test();
    let mut receiver = RawAudioReceiver(receiver);
    let _ = handle.tx.send(AsrStreamInput::RawF32(vec![0.25; 4096]));
    drop(handle);
    assert!(receiver.blocking_recv().is_err());
}

#[test]
fn dropping_raw_receiver_closes_its_capture_subscription() {
    let (sender, receiver) = RawAudioReceiver::channel();
    assert!(!sender.is_closed());
    drop(receiver);
    assert!(sender.is_closed());
    assert!(sender
        .send(AsrStreamInput::RawF32(vec![0.25; 4096]))
        .is_err());
}
