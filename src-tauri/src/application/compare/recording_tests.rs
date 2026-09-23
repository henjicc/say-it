use super::*;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

fn recording() -> (CompareRuntime, u64) {
    let runtime = CompareRuntime::default();
    let epoch = runtime.reset(vec![]);
    runtime.inner.lock().unwrap().phase = "recording".into();
    (runtime, epoch)
}

fn collect(rx: &mut UnboundedReceiver<AsrStreamInput>) -> Vec<f32> {
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
    senders: &[(&str, &UnboundedSender<AsrStreamInput>)],
) {
    for id in runtime.record_packet(epoch, samples).unwrap() {
        let (_, tx) = senders.iter().find(|(key, _)| *key == id).unwrap();
        tx.send(AsrStreamInput::RawF32(samples.to_vec())).unwrap();
    }
}

#[test]
fn late_consumers_receive_every_sample_once_and_only_file_models_keep_recording() {
    for keep_raw in [false, true] {
        let (runtime, epoch) = recording();
        let (a_tx, mut a_rx) = unbounded_channel();
        let (b_tx, mut b_rx) = unbounded_channel();
        let input: Vec<f32> = (0..12_397).map(|i| i as f32).collect();
        assert!(runtime
            .record_packet(epoch, &input[..4111])
            .unwrap()
            .is_empty());
        assert!(runtime
            .register_realtime_stream(epoch, "a".into(), 0, &a_tx)
            .unwrap());
        send_packet(&runtime, epoch, &input[4111..8333], &[("a", &a_tx)]);
        assert!(runtime
            .register_realtime_stream(epoch, "b".into(), 1, &b_tx)
            .unwrap());
        runtime
            .complete_stream_registration(epoch, keep_raw)
            .unwrap();
        if !keep_raw {
            assert_eq!(runtime.inner.lock().unwrap().raw.capacity(), 0);
        }
        send_packet(
            &runtime,
            epoch,
            &input[8333..],
            &[("a", &a_tx), ("b", &b_tx)],
        );
        assert_eq!(collect(&mut a_rx), input);
        assert_eq!(collect(&mut b_rx), input);
        let state = runtime.inner.lock().unwrap();
        if keep_raw {
            assert_eq!(state.raw, input);
        } else {
            assert_eq!(state.raw.capacity(), 0);
        }
    }
}

#[test]
fn concurrent_registration_cannot_reorder_backlog_and_live_audio() {
    for _ in 0..64 {
        let (runtime, epoch) = recording();
        let (tx, mut rx) = unbounded_channel();
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
    runtime.complete_stream_registration(stale, false).unwrap();
    assert!(runtime.record_packet(stale, &[0.5]).is_none());
    let (tx, mut rx) = unbounded_channel();
    assert!(!runtime
        .register_realtime_stream(stale, "old".into(), 0, &tx)
        .unwrap());
    assert!(rx.try_recv().is_err());
    assert_eq!(runtime.inner.lock().unwrap().raw, [0.25; 8193]);
    runtime.abort("test capture failure");
    assert_eq!(runtime.inner.lock().unwrap().raw.capacity(), 0);
    assert!(runtime.record_packet(epoch, &[0.5]).is_none());
}

#[test]
fn failed_stream_registration_keeps_audio_for_other_consumers() {
    let (runtime, epoch) = recording();
    runtime.record_packet(epoch, &[0.25; 8193]).unwrap();
    let (tx, rx) = unbounded_channel();
    drop(rx);
    assert!(runtime
        .register_realtime_stream(epoch, "failed".into(), 0, &tx)
        .is_err());
    assert!(runtime.inner.lock().unwrap().sessions.is_empty());
    let (tx, mut rx) = unbounded_channel();
    assert!(runtime
        .register_realtime_stream(epoch, "ok".into(), 1, &tx)
        .unwrap());
    assert_eq!(collect(&mut rx), [0.25; 8193]);
    runtime.complete_stream_registration(epoch, false).unwrap();
    assert_eq!(runtime.inner.lock().unwrap().raw.capacity(), 0);
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
            let (tx, mut rx) = unbounded_channel();
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
