use super::*;

#[cfg(windows)]
#[test]
#[ignore = "真实设备生命周期诊断；显式允许后短时采集，不保存、不发送音频"]
fn microphone_device_lifecycle_profile() {
    assert_eq!(
        std::env::var("SAYIT_ALLOW_MIC_LIFECYCLE").as_deref(),
        Ok("1")
    );
    use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessHandleCount};
    let state = RuntimeState::default();
    let mut rounds = Vec::new();
    for cycle in 0..12 {
        let started = std::time::Instant::now();
        let response = start_backend_mic_inner(None, &state).unwrap();
        let start_ms = started.elapsed().as_secs_f64() * 1000.0;
        assert!(!response.reused);
        // 没有识别接收者，采集回调只计算电平，既不缓存音频也不执行网络操作。
        std::thread::sleep(std::time::Duration::from_millis(100));
        let stopped = std::time::Instant::now();
        release_backend_mic_inner(&state).unwrap();
        let stop_ms = stopped.elapsed().as_secs_f64() * 1000.0;
        std::thread::sleep(std::time::Duration::from_millis(300));
        let mic = state.backend_mic.lock().unwrap();
        assert!(mic.worker.is_none());
        assert!(mic.raw_txs.is_empty() && mic.pending.is_empty());
        drop(mic);
        let mut handles = 0;
        unsafe {
            GetProcessHandleCount(GetCurrentProcess(), &mut handles).unwrap();
        }
        rounds.push(serde_json::json!({"cycle":cycle, "handles":handles,
            "privateBytes":crate::performance_test_support::memory().private_usage,
            "startMs":start_ms,"stopMs":stop_ms}));
    }
    println!(
        "PERF_RESULT {}",
        serde_json::json!({"scenario":"microphone-device-lifecycle","rounds":rounds})
    );
}

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

fn capture(preroll: AsrPreroll) -> (Arc<Mutex<BackendMicState>>, RawAudioReceiver) {
    let (tx, rx) = RawAudioReceiver::channel();
    (
        Arc::new(Mutex::new(BackendMicState {
            raw_txs: vec![BackendMicRawSubscriber { tx, preroll }],
            ..Default::default()
        })),
        rx,
    )
}

fn collect(rx: &mut RawAudioReceiver, count: usize) -> Vec<f32> {
    let mut result = Vec::new();
    while result.len() < count {
        result.extend(rx.blocking_recv().unwrap().unwrap());
    }
    assert_eq!(result.len(), count);
    result
}

fn collect_asr(rx: &mut AsrStreamReceiver, count: usize) -> Vec<f32> {
    let mut result = Vec::new();
    while result.len() < count {
        let Some(AsrStreamInput::RawF32(samples)) = rx.blocking_recv() else {
            panic!("识别输入提前结束")
        };
        result.extend(samples);
    }
    assert_eq!(result.len(), count);
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
            // 冻结的旧分包器会让尾包保留整段输入的容量；它原来使用无界通道。
            // 此处直接拼接旧尾包作为样本基准，避免让新通道的单包预算改写旧算法语义。
            let old_tail = std::mem::take(&mut old.lock().unwrap().buffer);
            let mut old_samples = collect(&mut old_rx, input.len() - old_tail.len());
            old_samples.extend(old_tail);
            flush_backend_mic_buffer(&mut new.lock().unwrap()).unwrap();
            assert_eq!(old_samples, input);
            assert_eq!(collect(&mut new_rx, input.len()), input);
        }
    }
}

#[test]
fn recording_consumers_need_no_duplicate_preroll_or_last_copy() {
    let (state, mut rx) = capture(AsrPreroll::Disabled);
    for i in 0..300 {
        push_backend_mic_samples(&state, vec![i as f32; 4096]);
        assert_eq!(collect(&mut rx, 4096), vec![i as f32; 4096]);
        assert!(state.lock().unwrap().pending.is_empty());
    }
    // 扇出最后一份直接转移分配，其余消费者仍得到独立且相同的样本。
    let (tx, mut first) = RawAudioReceiver::channel();
    let (tx2, mut last) = RawAudioReceiver::channel();
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
    assert_eq!(collect(&mut first, received.len()), received);
}

#[test]
fn reconnect_replays_same_bounded_history_before_live_tail_once() {
    let (state, mut raw) = capture(AsrPreroll::Enabled);
    for i in 0..250 {
        push_backend_mic_samples(&state, vec![i as f32; 4096]);
        assert_eq!(collect(&mut raw, 4096), vec![i as f32; 4096]);
    }
    assert_eq!(state.lock().unwrap().pending.len(), 240);
    push_backend_mic_samples(&state, vec![250.0; 17]);
    let (handle, mut asr) = AsrStreamHandle::channel();
    let tx = handle.tx;
    {
        let mut guard = state.lock().unwrap();
        guard.tx = Some(tx);
        flush_backend_mic_buffer(&mut guard).unwrap();
    }
    let mut expected: Vec<f32> = (10..250).flat_map(|i| vec![i as f32; 4096]).collect();
    expected.extend([250.0; 17]);
    assert_eq!(collect_asr(&mut asr, expected.len()), expected);
    assert_eq!(
        collect(&mut raw, 17),
        vec![250.0; 17],
        "预滚不能重复送给原始消费者"
    );
    push_backend_mic_samples(&state, vec![251.0; 4096]);
    assert_eq!(collect(&mut raw, 4096), vec![251.0; 4096]);
    assert_eq!(collect_asr(&mut asr, 4096), vec![251.0; 4096]);
    assert!(state.lock().unwrap().pending.is_empty());
    drop(asr);
    push_backend_mic_samples(&state, vec![252.0; 4096]);
    assert_eq!(collect(&mut raw, 4096), vec![252.0; 4096]);
    assert_eq!(
        state.lock().unwrap().pending.front().unwrap(),
        &vec![252.0; 4096]
    );
}

#[test]
fn closed_monitor_does_not_keep_recording_subscribers_in_preroll_mode() {
    let (state, mut recording) = capture(AsrPreroll::Disabled);
    let (tx, monitor) = RawAudioReceiver::channel();
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
    assert_eq!(collect(&mut recording, expected.len()), expected);
}

#[test]
fn failed_monitor_is_reported_and_does_not_interrupt_healthy_recording() {
    let (state, mut recording) = capture(AsrPreroll::Disabled);
    let (tx, mut monitor) = RawAudioReceiver::channel();
    assert!(tx.send(AsrStreamInput::RawF32(vec![0.0; 20_000])).is_err());
    state.lock().unwrap().raw_txs.push(BackendMicRawSubscriber {
        tx,
        preroll: AsrPreroll::Enabled,
    });
    push_backend_mic_samples(&state, vec![0.25; 4096]);
    assert_eq!(collect(&mut recording, 4096), vec![0.25; 4096]);
    assert!(monitor.blocking_recv().is_err());
    let state = state.lock().unwrap();
    assert_eq!(state.raw_txs.len(), 1);
    assert!(state.pending.is_empty());
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
    let mut consumed_packets = 0;
    let mut consume = |rx: &mut RawAudioReceiver, packets: usize| {
        loop {
            // 保留旧基准每次投喂后读取直到 Empty 的节奏；有磁盘票据时等到本轮包数齐全。
            let samples = match rx.try_recv() {
                Ok(AsrStreamInput::RawF32(samples)) => samples,
                Err(tokio::sync::mpsc::error::TryRecvError::Empty)
                    if consumed_packets < packets =>
                {
                    std::thread::yield_now();
                    continue;
                }
                Err(tokio::sync::mpsc::error::TryRecvError::Empty) => break,
                _ => panic!("采集基准丢失音频或提前结束"),
            };
            consumed_packets += 1;
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
        consume(&mut rx, state.lock().unwrap().chunk_count as usize);
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
    consume(&mut rx, state.lock().unwrap().chunk_count as usize);
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
