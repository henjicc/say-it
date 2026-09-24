use super::*;

// 冻结旧采集分包逻辑，仅用于输出对照与独立性能基线。
pub(crate) fn legacy_push(mic: &Arc<Mutex<BackendMicState>>, input: Vec<f32>) {
    if input.is_empty() {
        return;
    }
    let Ok(mut guard) = mic.lock() else {
        return;
    };
    guard.last_rms = rms_f32(&input);
    if guard.tx.is_none() && guard.session_id.is_none() && guard.raw_txs.is_empty() {
        return;
    }
    guard.buffer.extend_from_slice(&input);
    while guard.buffer.len() >= BACKEND_MIC_CHUNK_FRAMES {
        let chunk: Vec<f32> = guard.buffer.drain(..BACKEND_MIC_CHUNK_FRAMES).collect();
        guard.chunk_count += 1;
        guard.raw_txs.retain(|subscriber| {
            subscriber
                .tx
                .send(AsrStreamInput::RawF32(chunk.clone()))
                .is_ok()
        });
        if let Some(tx) = guard.tx.as_ref() {
            if tx.send(AsrStreamInput::RawF32(chunk.clone())).is_ok() {
                continue;
            }
            guard.tx = None;
            guard.session_id = None;
        }
        guard.pending.push_back(chunk);
        while guard.pending.len() > 240 {
            guard.pending.pop_front();
        }
    }
}

fn capture(
    preroll: AsrPreroll,
) -> (
    Arc<Mutex<BackendMicState>>,
    tokio::sync::mpsc::UnboundedReceiver<AsrStreamInput>,
) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    (
        Arc::new(Mutex::new(BackendMicState {
            raw_txs: vec![BackendMicRawSubscriber { tx, preroll }],
            ..Default::default()
        })),
        rx,
    )
}

fn collect(rx: &mut tokio::sync::mpsc::UnboundedReceiver<AsrStreamInput>) -> Vec<f32> {
    let mut result = Vec::new();
    while let Ok(AsrStreamInput::RawF32(samples)) = rx.try_recv() {
        result.extend(samples);
    }
    result
}

#[test]
fn chunking_matches_original_for_device_boundaries_and_large_packets() {
    let input: Vec<f32> = (0..480_017)
        .map(|i| (i % 997) as f32 / 997.0 - 0.5)
        .collect();
    for packet in [1, 479, 480, 4096, 4097, 480_000] {
        for preroll in [AsrPreroll::Enabled, AsrPreroll::Disabled] {
            let (old, mut old_rx) = capture(preroll);
            let (new, mut new_rx) = capture(preroll);
            for part in input.chunks(packet) {
                legacy_push(&old, part.to_vec());
                push_backend_mic_samples(&new, part.to_vec());
                assert!(new.lock().unwrap().buffer.capacity() <= BACKEND_MIC_CHUNK_FRAMES);
            }
            flush_backend_mic_buffer(&mut old.lock().unwrap()).unwrap();
            flush_backend_mic_buffer(&mut new.lock().unwrap()).unwrap();
            assert_eq!(collect(&mut old_rx), input);
            assert_eq!(collect(&mut new_rx), input);
        }
    }
}

#[test]
fn recording_consumers_need_no_duplicate_preroll_or_last_copy() {
    let (state, mut rx) = capture(AsrPreroll::Disabled);
    for i in 0..300 {
        push_backend_mic_samples(&state, vec![i as f32; 4096]);
        assert_eq!(collect(&mut rx), vec![i as f32; 4096]);
        assert!(state.lock().unwrap().pending.is_empty());
    }
    // 扇出最后一份直接转移分配，其余消费者仍得到独立且相同的样本。
    let (tx, mut first) = tokio::sync::mpsc::unbounded_channel();
    let (tx2, mut last) = tokio::sync::mpsc::unbounded_channel();
    let samples = vec![0.25; 4096];
    let original = samples.as_ptr();
    let mut subscribers = vec![
        BackendMicRawSubscriber {
            tx,
            preroll: AsrPreroll::Disabled,
        },
        BackendMicRawSubscriber {
            tx: tx2,
            preroll: AsrPreroll::Disabled,
        },
    ];
    assert!(fanout_raw(&mut subscribers, samples, false).0.is_none());
    let AsrStreamInput::RawF32(received) = last.try_recv().unwrap() else {
        panic!()
    };
    assert_eq!(received.as_ptr(), original);
    assert_eq!(collect(&mut first), received);
}

#[test]
fn reconnect_replays_same_bounded_history_before_live_tail_once() {
    let (state, mut raw) = capture(AsrPreroll::Enabled);
    for i in 0..250 {
        push_backend_mic_samples(&state, vec![i as f32; 4096]);
        assert_eq!(collect(&mut raw), vec![i as f32; 4096]);
    }
    assert_eq!(state.lock().unwrap().pending.len(), 240);
    push_backend_mic_samples(&state, vec![250.0; 17]);
    let (tx, mut asr) = tokio::sync::mpsc::unbounded_channel();
    {
        let mut guard = state.lock().unwrap();
        guard.tx = Some(tx);
        flush_backend_mic_buffer(&mut guard).unwrap();
    }
    let mut expected: Vec<f32> = (10..250).flat_map(|i| vec![i as f32; 4096]).collect();
    expected.extend([250.0; 17]);
    assert_eq!(collect(&mut asr), expected);
    assert_eq!(
        collect(&mut raw),
        vec![250.0; 17],
        "预滚不能重复送给原始消费者"
    );
    push_backend_mic_samples(&state, vec![251.0; 4096]);
    assert_eq!(collect(&mut raw), vec![251.0; 4096]);
    assert_eq!(collect(&mut asr), vec![251.0; 4096]);
    assert!(state.lock().unwrap().pending.is_empty());
    drop(asr);
    push_backend_mic_samples(&state, vec![252.0; 4096]);
    assert_eq!(collect(&mut raw), vec![252.0; 4096]);
    assert_eq!(
        state.lock().unwrap().pending.front().unwrap(),
        &vec![252.0; 4096]
    );
}

#[test]
fn closed_monitor_does_not_keep_recording_subscribers_in_preroll_mode() {
    let (state, mut recording) = capture(AsrPreroll::Disabled);
    let (tx, monitor) = tokio::sync::mpsc::unbounded_channel();
    state.lock().unwrap().raw_txs.push(BackendMicRawSubscriber {
        tx,
        preroll: AsrPreroll::Enabled,
    });
    push_backend_mic_samples(&state, vec![0.1; 4096]);
    assert_eq!(state.lock().unwrap().pending.len(), 1);
    drop(monitor);
    push_backend_mic_samples(&state, vec![0.2; 4096]);
    assert!(state.lock().unwrap().pending.is_empty());
    let mut expected = vec![0.1; 4096];
    expected.extend([0.2; 4096]);
    assert_eq!(collect(&mut recording), expected);
}

#[cfg(windows)]
#[test]
#[ignore = "采集层本地合成基准，不打开设备或识别模型"]
fn capture_memory_profile() {
    use crate::performance_test_support::memory;
    use std::time::Instant;
    let seconds = std::env::var("SAYIT_PERF_AUDIO_SECONDS")
        .unwrap_or("300".into())
        .parse::<usize>()
        .unwrap();
    let packet = std::env::var("SAYIT_PERF_PACKET_SIZE")
        .unwrap_or("480".into())
        .parse::<usize>()
        .unwrap();
    assert!((1..=1800).contains(&seconds) && (1..=2_880_000).contains(&packet));
    let legacy = std::env::var("SAYIT_PERF_CAPTURE_LEGACY").as_deref() == Ok("1");
    let (state, mut rx) = capture(AsrPreroll::Disabled);
    let initial = memory();
    let started = Instant::now();
    let total = seconds * 48_000;
    let mut hash = 0xcbf29ce484222325_u64;
    let mut count = 0;
    let mut consume = |rx: &mut tokio::sync::mpsc::UnboundedReceiver<AsrStreamInput>| {
        while let Ok(AsrStreamInput::RawF32(samples)) = rx.try_recv() {
            count += samples.len();
            for sample in samples {
                hash = (hash ^ sample.to_bits() as u64).wrapping_mul(0x100000001b3);
            }
        }
    };
    for offset in (0..total).step_by(packet) {
        let input: Vec<f32> = (offset..(offset + packet).min(total))
            .map(|i| (i % 997) as f32 / 997.0 - 0.5)
            .collect();
        if legacy {
            legacy_push(&state, input);
        } else {
            push_backend_mic_samples(&state, input);
        }
        consume(&mut rx);
    }
    let before_flush = memory();
    let retained_samples = state
        .lock()
        .unwrap()
        .pending
        .iter()
        .map(Vec::len)
        .sum::<usize>();
    let buffer_capacity = state.lock().unwrap().buffer.capacity();
    flush_backend_mic_buffer(&mut state.lock().unwrap()).unwrap();
    consume(&mut rx);
    let elapsed = started.elapsed();
    let after = memory();
    assert_eq!(count, total);
    println!(
        "PERF_RESULT {}",
        serde_json::json!({
            "scenario": "capture-raw-recording", "legacy": legacy, "seconds": seconds, "packetSize":packet,
            "elapsedMs":elapsed.as_secs_f64()*1000.0, "outputHash":format!("{hash:016x}"), "samples":count,
            "retainedPrerollSamples":retained_samples, "bufferCapacity":buffer_capacity,
            "initialPrivateBytes":initial.private_usage, "recordingPrivateBytes":before_flush.private_usage,
            "peakPrivateBytes":after.peak_pagefile_usage, "peakWorkingSetBytes":after.peak_working_set,
            "releasedPrivateBytes":after.private_usage
        })
    );
}
