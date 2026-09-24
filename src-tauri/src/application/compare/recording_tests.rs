use super::*;
use crate::state::{AsrInputSender, AsrStreamHandle, AsrStreamReceiver};
fn input_channel() -> (AsrInputSender, AsrStreamReceiver) {
    let (handle, rx) = AsrStreamHandle::channel();
    (handle.tx, rx)
}

fn recording() -> (CompareRuntime, u64) {
    let runtime = CompareRuntime::default();
    let epoch = runtime.reset(vec![]);
    runtime.inner.lock().unwrap().phase = "recording".into();
    (runtime, epoch)
}

fn collect(rx: &mut AsrStreamReceiver) -> Vec<f32> {
    let mut output = Vec::new();
    while let Ok(packet) = rx.try_recv() {
        let AsrStreamInput::RawF32(samples) = packet else {
            panic!("expected audio")
        };
        output.extend(samples);
    }
    output
}

fn send_packet(
    runtime: &CompareRuntime,
    epoch: u64,
    samples: &[f32],
    senders: &[(&str, &AsrInputSender)],
) {
    for id in runtime.record_packet(epoch, samples).unwrap().unwrap() {
        let (_, tx) = senders.iter().find(|(key, _)| *key == id).unwrap();
        tx.send(AsrStreamInput::RawF32(samples.to_vec())).unwrap();
    }
}

#[test]
fn late_consumers_receive_every_sample_once_and_file_models_store_identical_wav() {
    for needs_file in [false, true] {
        let (runtime, epoch) = recording();
        let (a_tx, mut a_rx) = input_channel();
        let (b_tx, mut b_rx) = input_channel();
        let input: Vec<f32> = (0..12_397)
            .map(|i| (i as f32 / 12397.0 - 0.5) * 3.0)
            .collect();
        let mut recording =
            needs_file.then(|| WavRecording::new(44_100, Quantization::Truncate).unwrap());
        if let Some(writer) = recording.as_mut() {
            writer.append(&input[..8333]).unwrap();
        }
        assert!(runtime
            .record_packet(epoch, &input[..4111])
            .unwrap()
            .unwrap()
            .is_empty());
        assert!(runtime
            .register_realtime_stream(epoch, "a".into(), 0, &a_tx)
            .unwrap());
        send_packet(&runtime, epoch, &input[4111..8333], &[("a", &a_tx)]);
        assert!(runtime
            .register_realtime_stream(epoch, "b".into(), 1, &b_tx)
            .unwrap());
        runtime.complete_stream_registration(epoch).unwrap();
        assert!(runtime.inner.lock().unwrap().raw.is_released());
        send_packet(
            &runtime,
            epoch,
            &input[8333..],
            &[("a", &a_tx), ("b", &b_tx)],
        );
        assert_eq!(collect(&mut a_rx), input);
        assert_eq!(collect(&mut b_rx), input);
        let state = runtime.inner.lock().unwrap();
        assert!(state.raw.is_released());
        if let Some(mut writer) = recording {
            writer.append(&input[8333..]).unwrap();
            let file = writer.finish().unwrap();
            let expected = write_wav(&input, 44_100).unwrap();
            assert_eq!(
                std::fs::read(file.path()).unwrap(),
                std::fs::read(&expected).unwrap()
            );
            std::fs::remove_file(expected).unwrap();
        }
    }
}

#[test]
fn concurrent_registration_cannot_reorder_backlog_and_live_audio() {
    for _ in 0..64 {
        let (runtime, epoch) = recording();
        let (tx, mut rx) = input_channel();
        runtime.record_packet(epoch, &[1.0, 2.0]).unwrap();
        let barrier = std::sync::Barrier::new(2);
        std::thread::scope(|scope| {
            scope.spawn(|| {
                barrier.wait();
                runtime
                    .register_realtime_stream(epoch, "a".into(), 0, &tx)
                    .unwrap();
            });
            scope.spawn(|| {
                barrier.wait();
                send_packet(&runtime, epoch, &[3.0, 4.0], &[("a", &tx)]);
            });
        });
        assert_eq!(collect(&mut rx), [1.0, 2.0, 3.0, 4.0]);
    }
}

#[test]
fn stale_start_and_capture_do_not_touch_a_new_recording() {
    let (runtime, stale) = recording();
    let epoch = runtime.reset(vec![]);
    runtime.inner.lock().unwrap().phase = "recording".into();
    runtime.record_packet(epoch, &[0.25; 8193]).unwrap();
    runtime.complete_stream_registration(stale).unwrap();
    assert!(runtime.record_packet(stale, &[0.5]).unwrap().is_none());
    let (tx, mut rx) = input_channel();
    assert!(!runtime
        .register_realtime_stream(stale, "old".into(), 0, &tx)
        .unwrap());
    assert!(rx.try_recv().is_err());
    assert_eq!(runtime.inner.lock().unwrap().raw.to_vec(), [0.25; 8193]);
    runtime.abort("test capture failure");
    assert!(runtime.inner.lock().unwrap().raw.is_released());
    assert!(runtime.record_packet(epoch, &[0.5]).unwrap().is_none());
}

#[test]
fn failed_stream_registration_keeps_audio_for_other_consumers() {
    let (runtime, epoch) = recording();
    runtime.record_packet(epoch, &[0.25; 8193]).unwrap();
    let (tx, rx) = input_channel();
    drop(rx);
    assert!(runtime
        .register_realtime_stream(epoch, "failed".into(), 0, &tx)
        .is_err());
    assert!(runtime.inner.lock().unwrap().sessions.is_empty());
    let (tx, mut rx) = input_channel();
    assert!(runtime
        .register_realtime_stream(epoch, "ok".into(), 1, &tx)
        .unwrap());
    assert_eq!(collect(&mut rx), [0.25; 8193]);
    runtime.complete_stream_registration(epoch).unwrap();
    assert!(runtime.inner.lock().unwrap().raw.is_released());
}

#[test]
fn chunked_startup_replay_preserves_realtime_dsp_bytes() {
    for rate in [16_000, 44_100, 48_000] {
        for denoise in [false, true] {
            let input: Vec<f32> = (0..rate * 2 + 197)
                .map(|i| ((i * 1777 % 65535) as f32 - 32767.0) / 32768.0)
                .collect();
            let (runtime, epoch) = recording();
            runtime.record_packet(epoch, &input).unwrap();
            let (tx, mut rx) = input_channel();
            runtime
                .register_realtime_stream(epoch, "a".into(), 0, &tx)
                .unwrap();
            let params = DspParams {
                denoise_enabled: denoise,
                denoise_strength: 0.8,
                vad_gate: 0.01,
                bass_gain_db: 3.0,
                treble_gain_db: -2.0,
                ..Default::default()
            };
            let reference = crate::audio_dsp::StreamDsp::new(params.clone(), rate).process(&input);
            let mut dsp = crate::audio_dsp::StreamDsp::new(params, rate);
            let mut actual = Vec::new();
            while let Ok(packet) = rx.try_recv() {
                let AsrStreamInput::RawF32(samples) = packet else {
                    unreachable!()
                };
                assert!(samples.len() <= 4096);
                actual.extend(dsp.process(&samples));
            }
            assert_eq!(actual, reference, "rate={rate}, denoise={denoise}");
        }
    }
}

#[test]
fn stop_waits_for_tail_and_preserves_file_until_last_consumer_exits() {
    tauri::async_runtime::block_on(async {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let worker = tauri::async_runtime::spawn_blocking(move || {
            let mut writer = WavRecording::new(48_000, Quantization::Truncate).unwrap();
            writer.append(&[0.25; 4096]).unwrap();
            writer.append(&[-0.75; 17]).unwrap();
            assert!(tx.send(Ok(Some(writer.finish().unwrap()))).is_ok());
        });
        let file = Arc::new(drain_recording(Some(rx)).await.unwrap().unwrap());
        worker.await.unwrap();
        assert_eq!(file.samples, 4113);
        let path = file.path().to_owned();
        let first = file.clone();
        let second = file.clone();
        drop(first);
        assert!(path.exists(), "首个任务提前完成不能删除其他任务的输入");
        drop(file);
        assert!(path.exists(), "取消登记方不能删除仍在处理的输入");
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(bytes.len(), 44 + 4113 * 2);
        assert_eq!(&bytes[bytes.len() - 2..], &(-24575i16).to_le_bytes());
        drop(second);
        assert!(!path.exists(), "最后的实际消费者退出后删除");
    });
}

#[test]
fn cancelled_drain_and_reset_drop_completed_recording_without_leaking() {
    for cancel_before_send in [false, true] {
        let (runtime, _) = recording();
        let (tx, rx) = tokio::sync::oneshot::channel();
        runtime.inner.lock().unwrap().recording_drain = Some(rx);
        let file = WavRecording::new(48_000, Quantization::Truncate)
            .unwrap()
            .finish()
            .unwrap();
        let path = file.path().to_owned();
        if cancel_before_send {
            runtime.abort("取消");
            assert!(tx.is_closed());
            drop(tx.send(Ok(Some(file))));
        } else {
            assert!(tx.send(Ok(Some(file))).is_ok());
            runtime.reset(vec![]);
        }
        assert!(!path.exists());
    }
}

#[test]
fn drain_failure_never_accepts_a_partial_recording() {
    tauri::async_runtime::block_on(async {
        assert!(drain_recording(None).await.is_err());
        let (tx, rx) = tokio::sync::oneshot::channel();
        drop(tx);
        assert!(drain_recording(Some(rx)).await.is_err());
        let (tx, rx) = tokio::sync::oneshot::channel();
        assert!(tx.send(Err("磁盘已满".into())).is_ok());
        assert_eq!(drain_recording(Some(rx)).await.err().unwrap(), "磁盘已满");
        let (tx, rx) = tokio::sync::oneshot::channel();
        assert!(tx.send(Ok(None)).is_ok());
        assert!(
            drain_recording(Some(rx)).await.unwrap().is_none(),
            "纯实时模式无需录音文件"
        );
    });
}

#[test]
fn spilled_startup_replay_can_catch_up_while_capture_continues() {
    for _ in 0..8 {
        let (runtime, epoch) = recording();
        let (tx, mut rx) = input_channel();
        let initial = vec![0.25; 600_017];
        runtime.record_packet(epoch, &initial).unwrap();
        let barrier = std::sync::Barrier::new(2);
        std::thread::scope(|scope| {
            let received = scope.spawn(move || {
                let mut output = Vec::new();
                while let Some(packet) = rx.blocking_recv() {
                    match packet {
                        AsrStreamInput::RawF32(samples) => output.extend(samples),
                        AsrStreamInput::Finish => return output,
                        _ => panic!("unexpected failure or stop"),
                    }
                }
                panic!("missing finish")
            });
            let registration = scope.spawn(|| {
                barrier.wait();
                assert!(runtime
                    .register_realtime_stream(epoch, "a".into(), 0, &tx)
                    .unwrap());
            });
            barrier.wait();
            for i in 0..256 {
                send_packet(&runtime, epoch, &[i as f32; 4096], &[("a", &tx)]);
            }
            registration.join().unwrap();
            runtime.complete_stream_registration(epoch).unwrap();
            send_packet(&runtime, epoch, &[-0.75; 17], &[("a", &tx)]);
            tx.send(AsrStreamInput::Finish).unwrap();
            let output = received.join().unwrap();
            assert_eq!(&output[..initial.len()], initial);
            for (i, part) in output[initial.len()..output.len() - 17]
                .chunks(4096)
                .enumerate()
            {
                assert!(part.iter().all(|sample| *sample == i as f32));
            }
            assert_eq!(output.len(), initial.len() + 256 * 4096 + 17);
            assert_eq!(&output[output.len() - 17..], &[-0.75; 17]);
            assert!(runtime.inner.lock().unwrap().raw.is_released());
        });
    }
}

#[test]
fn cancellation_during_spilled_replay_never_registers_the_old_stream() {
    let (runtime, epoch) = recording();
    runtime
        .record_packet(epoch, &vec![0.25; 2_000_017])
        .unwrap();
    let (tx, mut rx) = input_channel();
    std::thread::scope(|scope| {
        let replay = scope.spawn(|| runtime.register_realtime_stream(epoch, "old".into(), 0, &tx));
        assert!(matches!(
            rx.blocking_recv(),
            Some(AsrStreamInput::RawF32(_))
        ));
        runtime.epoch.fetch_add(1, Ordering::AcqRel);
        runtime.abort("已取消");
        // 无论是否已追上，取消后的登记集合必须为空；旧工作器不能再次接入。
        let _ = replay.join().unwrap();
        assert!(runtime.inner.lock().unwrap().sessions.is_empty());
        assert!(runtime.inner.lock().unwrap().raw.is_released());
        let next = runtime.reset(vec![]);
        runtime.inner.lock().unwrap().phase = "recording".into();
        runtime.record_packet(next, &[0.5]).unwrap();
        assert_eq!(runtime.inner.lock().unwrap().raw.to_vec(), [0.5]);
    });
}
