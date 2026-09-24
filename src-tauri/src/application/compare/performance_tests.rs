//! 单独进程测量真实导出路径；不启动模型或网络请求。
use super::*;
use crate::performance_test_support::memory;
use std::io::Read;
use std::time::Instant;

#[test]
#[ignore = "独立模型启动缓存测量；合成音频，不启动模型、设备或网络"]
fn startup_storage_profile() {
    use windows::Win32::System::Threading::{
        GetCurrentProcess, GetProcessHandleCount, GetProcessIoCounters, IO_COUNTERS,
    };
    let counters = || {
        let mut io = IO_COUNTERS::default();
        let mut handles = 0;
        unsafe {
            GetProcessIoCounters(GetCurrentProcess(), &mut io).unwrap();
            GetProcessHandleCount(GetCurrentProcess(), &mut handles).unwrap();
        }
        (io, handles)
    };
    let seconds = std::env::var("SAYIT_PERF_AUDIO_SECONDS")
        .unwrap_or("300".into())
        .parse::<usize>()
        .unwrap();
    assert!((1..=1800).contains(&seconds));
    let legacy = std::env::var("SAYIT_PERF_STARTUP_LEGACY").as_deref() == Ok("1");
    let runtime = CompareRuntime::default();
    let epoch = runtime.reset(vec![]);
    runtime.inner.lock().unwrap().phase = "recording".into();
    let input: Vec<f32> = (0..4096).map(|i| (i % 997) as f32 / 996.0 - 0.5).collect();
    let total = seconds * 48_000;
    let initial = memory();
    let (io_before, handles_before) = counters();
    let started = Instant::now();
    let mut old_raw = Vec::new();
    for offset in (0..total).step_by(input.len()) {
        let part = &input[..(total - offset).min(input.len())];
        if legacy {
            // 冻结旧版的 Vec 追加；不要让新存储实现渗入对照组。
            old_raw.extend_from_slice(part);
        } else {
            assert!(runtime
                .record_packet(epoch, part)
                .unwrap()
                .unwrap()
                .is_empty());
        }
    }
    let recorded = started.elapsed();
    let retained = memory();
    let (_, handles_retained) = counters();
    let mut hash = 0xcbf29ce484222325_u64;
    let mut count = 0;
    let mut consume = |part: &[f32]| {
        count += part.len();
        for sample in part {
            hash = (hash ^ sample.to_bits() as u64).wrapping_mul(0x100000001b3);
        }
    };
    let replay_started = Instant::now();
    if legacy {
        for part in old_raw.chunks(4096) {
            consume(part);
        }
    } else {
        let raw = runtime.inner.lock().unwrap().raw.snapshot();
        let mut block = [0f32; 4096];
        for start in (0..raw.len()).step_by(block.len()) {
            let count = (raw.len() - start).min(block.len());
            raw.read(start, &mut block[..count]).unwrap();
            consume(&block[..count]);
        }
    }
    let replay = replay_started.elapsed();
    assert_eq!(count, total);
    runtime.complete_stream_registration(epoch).unwrap();
    drop(old_raw);
    let elapsed = started.elapsed();
    let (io_after, handles_after) = counters();
    let released = memory();
    assert_eq!(handles_before, handles_after);
    println!(
        "PERF_RESULT {}",
        serde_json::json!({
            "scenario":"compare-startup-storage", "legacy":legacy, "seconds":seconds,
            "elapsedMs":elapsed.as_secs_f64()*1000.0, "recordMs":recorded.as_secs_f64()*1000.0,
            "readAndHashMs":replay.as_secs_f64()*1000.0, "samples":count,
            "outputHash":format!("{hash:016x}"), "initialPrivateBytes":initial.private_usage,
            "retainedPrivateBytes":retained.private_usage, "releasedPrivateBytes":released.private_usage,
            "peakPrivateBytes":released.peak_pagefile_usage, "peakWorkingSetBytes":released.peak_working_set,
            "readBytes":io_after.ReadTransferCount-io_before.ReadTransferCount,
            "writeBytes":io_after.WriteTransferCount-io_before.WriteTransferCount,
            "handlesBefore":handles_before,"handlesRetained":handles_retained,"handlesAfter":handles_after,
        })
    );
}

#[test]
#[ignore = "独立性能采样：本地合成文件，不调用识别服务"]
fn uploaded_playback_memory_profile() {
    use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessIoCounters, IO_COUNTERS};
    let io = || {
        let mut counters = IO_COUNTERS::default();
        unsafe {
            GetProcessIoCounters(GetCurrentProcess(), &mut counters).unwrap();
        }
        counters
    };
    let seconds = std::env::var("SAYIT_PERF_AUDIO_SECONDS")
        .unwrap_or("300".into())
        .parse::<usize>()
        .unwrap();
    assert!((1..=1800).contains(&seconds));
    let legacy = std::env::var("SAYIT_PERF_PLAYBACK_LEGACY").as_deref() == Ok("1");
    let path =
        std::env::temp_dir().join(format!("say-it-playback-perf-{}.wav", uuid::Uuid::new_v4()));
    crate::audio_prep::write_test_stereo_wav(&path, seconds as f32, 48_000);
    tauri::async_runtime::block_on(async {});
    let initial = memory();
    let io_before = io();
    let started = Instant::now();
    let mut hash = 0xcbf29ce484222325_u64;
    let mut received = 0u64;
    let mut consume = |part: &[f32]| {
        assert!(part.len() <= playback::PACKET_SAMPLES);
        received += part.len() as u64;
        for sample in part {
            hash = (hash ^ sample.to_bits() as u64).wrapping_mul(0x100000001b3);
        }
    };
    let prepared;
    let total;
    if legacy {
        let samples = crate::audio_prep::decode_to_mono_16k(path.to_str().unwrap()).unwrap();
        total = samples.len() as u64;
        prepared = started.elapsed();
        for part in samples.chunks(1600) {
            consume(part);
        }
    } else {
        total = playback::inspect(path.to_str().unwrap(), || Ok(())).unwrap();
        prepared = started.elapsed();
        let (mut rx, worker) = playback::start(path.to_str().unwrap().to_owned(), || Ok(()));
        while let Some(packet) = rx.blocking_recv() {
            consume(&packet);
        }
        assert_eq!(
            tauri::async_runtime::block_on(worker).unwrap().unwrap(),
            total
        );
    }
    let elapsed = started.elapsed();
    let io_after = io();
    let after = memory();
    std::fs::remove_file(path).unwrap();
    assert_eq!(received, total);
    assert_eq!(total, seconds as u64 * 16_000);
    println!(
        "PERF_RESULT {}",
        serde_json::json!({
            "scenario": "compare-uploaded-playback", "legacy": legacy, "seconds": seconds,
            "elapsedMs": elapsed.as_secs_f64() * 1000.0, "preparedMs": prepared.as_secs_f64() * 1000.0,
            "outputHash": format!("{hash:016x}"), "samples": total,
            "readBytes": io_after.ReadTransferCount - io_before.ReadTransferCount,
            "writeBytes": io_after.WriteTransferCount - io_before.WriteTransferCount,
            "initialPrivateBytes": initial.private_usage, "retainedPrivateBytes": after.private_usage,
            "peakPrivateBytes": after.peak_pagefile_usage, "peakWorkingSetBytes": after.peak_working_set,
        })
    );
}

#[test]
#[ignore = "独立性能采样：实时对比录音，仅本地接收器，不启动模型"]
fn realtime_recording_memory_profile() {
    let seconds = std::env::var("SAYIT_PERF_AUDIO_SECONDS")
        .ok()
        .map(|value| value.parse::<usize>().unwrap())
        .unwrap_or(300);
    assert!((1..=1800).contains(&seconds));
    let initial = memory();
    let runtime = CompareRuntime::default();
    let epoch = runtime.reset(vec![]);
    let mut sinks = Vec::new();
    {
        let mut state = runtime.inner.lock().unwrap();
        state.phase = "recording".into();
        for index in 0..3 {
            let id = format!("local-test-{index}");
            state.sessions.insert(id.clone(), index);
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<AsrStreamInput>();
            sinks.push((id, tx, rx, 0xcbf29ce484222325_u64, 0usize));
        }
    }
    runtime.complete_stream_registration(epoch).unwrap();
    let chunk: Vec<f32> = (0..4096).map(|i| (i as f32 / 4096.0 - 0.5) * 0.3).collect();
    let total = seconds * 48_000;
    let started = Instant::now();
    for offset in (0..total).step_by(chunk.len()) {
        let part = &chunk[..(total - offset).min(chunk.len())];
        let sessions = runtime.record_packet(epoch, part).unwrap().unwrap();
        for id in sessions {
            let (_, tx, rx, hash, count) = sinks.iter_mut().find(|(key, ..)| *key == id).unwrap();
            tx.send(AsrStreamInput::RawF32(part.to_vec())).unwrap();
            let AsrStreamInput::RawF32(received) = rx.try_recv().unwrap() else {
                panic!("应收到音频")
            };
            assert_eq!(received.len(), part.len());
            for sample in received {
                *hash = (*hash ^ sample.to_bits() as u64).wrapping_mul(0x100000001b3);
            }
            *count += part.len();
        }
    }
    let elapsed = started.elapsed();
    let after = memory();
    let hash = sinks[0].3;
    for (_, _, _, actual, count) in &sinks {
        assert_eq!(*actual, hash);
        assert_eq!(*count, total);
    }
    let retained_samples = runtime.inner.lock().unwrap().raw.len();
    drop(runtime);
    drop(sinks);
    let released = memory();
    println!(
        "PERF_RESULT {}",
        serde_json::json!({
            "scenario": "compare-realtime-recording", "seconds": seconds,
            "elapsedMs": elapsed.as_secs_f64() * 1000.0,
            "outputHash": format!("{hash:016x}"), "samplesPerSink": total, "sinks": 3,
            "retainedSamples": retained_samples,
            "initialPrivateBytes": initial.private_usage, "retainedPrivateBytes": after.private_usage,
            "peakPrivateBytes": after.peak_pagefile_usage, "peakWorkingSetBytes": after.peak_working_set,
            "releasedPrivateBytes": released.private_usage,
        })
    );
}

#[test]
#[ignore = "独立性能采样：release 模式、单用例、单线程"]
fn wav_export_memory_profile() {
    let seconds = std::env::var("SAYIT_PERF_AUDIO_SECONDS")
        .ok()
        .map(|v| v.parse::<usize>().expect("音频时长必须是整数"))
        .unwrap_or(300);
    assert!((1..=1800).contains(&seconds));
    let initial = memory();
    let samples: Vec<f32> = (0..seconds * 48_000)
        .map(|i| ((i % 997) as f32 / 996.0 - 0.5) * 2.5)
        .collect();
    let before = memory();
    let started = Instant::now();
    let path = write_wav(&samples, 48_000).unwrap();
    let elapsed = started.elapsed();
    let after = memory();
    let mut file = std::fs::File::open(&path).unwrap();
    let output_bytes = file.metadata().unwrap().len();
    assert_eq!(output_bytes, 44 + samples.len() as u64 * 2);
    let mut buffer = [0u8; 64 * 1024];
    let mut hash = 0xcbf29ce484222325_u64;
    loop {
        let n = file.read(&mut buffer).unwrap();
        if n == 0 {
            break;
        }
        for byte in &buffer[..n] {
            hash = (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
        }
    }
    drop(file);
    std::fs::remove_file(path).unwrap();
    drop(samples);
    let released = memory();
    println!(
        "PERF_RESULT {}",
        serde_json::json!({
            "scenario": "compare-wav-export",
            "seconds": seconds,
            "elapsedMs": elapsed.as_secs_f64() * 1000.0,
            "outputBytes": output_bytes,
            "outputHash": format!("{hash:016x}"),
            "initialPrivateBytes": initial.private_usage,
            "inputPrivateBytes": before.private_usage,
            "retainedPrivateBytes": after.private_usage,
            "peakPrivateBytes": after.peak_pagefile_usage,
            "peakWorkingSetBytes": after.peak_working_set,
            "releasedPrivateBytes": released.private_usage,
        })
    );
}

#[test]
#[ignore = "独立文件模型录音存储测量，不启动识别或麦克风"]
fn file_recording_storage_profile() {
    use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessIoCounters, IO_COUNTERS};
    let io = || {
        let mut counters = IO_COUNTERS::default();
        unsafe {
            GetProcessIoCounters(GetCurrentProcess(), &mut counters).unwrap();
        }
        counters
    };
    let seconds = std::env::var("SAYIT_PERF_AUDIO_SECONDS")
        .unwrap_or("300".into())
        .parse::<usize>()
        .unwrap();
    assert!((1..=1800).contains(&seconds));
    let legacy = std::env::var("SAYIT_PERF_RECORDING_LEGACY").as_deref() == Ok("1");
    let input: Vec<f32> = (0..4096).map(|i| (i % 997) as f32 / 996.0 - 0.5).collect();
    let runtime = CompareRuntime::default();
    let epoch = runtime.reset(vec![]);
    runtime.inner.lock().unwrap().phase = "recording".into();
    if !legacy {
        runtime.complete_stream_registration(epoch).unwrap();
    }
    let initial = memory();
    let io_before = io();
    let started = Instant::now();
    let mut legacy_raw = Vec::new();
    let mut recording =
        (!legacy).then(|| WavRecording::new(48_000, Quantization::Truncate).unwrap());
    for offset in (0..seconds * 48_000).step_by(input.len()) {
        let part = &input[..(seconds * 48_000 - offset).min(input.len())];
        if legacy {
            legacy_raw.extend_from_slice(part);
        } else {
            assert!(runtime
                .record_packet(epoch, part)
                .unwrap()
                .unwrap()
                .is_empty());
        }
        if let Some(writer) = recording.as_mut() {
            writer.append(part).unwrap();
        }
    }
    let retained = memory();
    let stop_started = Instant::now();
    let owned;
    let path;
    if legacy {
        path = std::path::PathBuf::from(write_wav(&legacy_raw, 48_000).unwrap());
        drop(legacy_raw);
        owned = None;
    } else {
        let file = recording.unwrap().finish().unwrap();
        assert_eq!(file.samples, seconds * 48_000);
        path = file.path().to_owned();
        owned = Some(file);
        assert!(runtime.inner.lock().unwrap().raw.is_released());
    }
    let elapsed = started.elapsed();
    let stop_elapsed = stop_started.elapsed();
    let io_after = io();
    let after = memory();
    let mut reader = std::fs::File::open(&path).unwrap();
    let bytes = reader.metadata().unwrap().len();
    assert_eq!(bytes, 44 + seconds as u64 * 48_000 * 2);
    let mut buffer = [0u8; 64 * 1024];
    let mut hash = 0xcbf29ce484222325_u64;
    loop {
        let n = reader.read(&mut buffer).unwrap();
        if n == 0 {
            break;
        }
        for byte in &buffer[..n] {
            hash = (hash ^ *byte as u64).wrapping_mul(0x100000001b3);
        }
    }
    drop(reader);
    drop(owned);
    if legacy {
        std::fs::remove_file(&path).unwrap();
    }
    assert!(!path.exists());
    println!(
        "PERF_RESULT {}",
        serde_json::json!({
            "scenario": "compare-file-recording-storage", "legacy": legacy, "seconds": seconds,
            "elapsedMs": elapsed.as_secs_f64() * 1000.0, "stopMs": stop_elapsed.as_secs_f64() * 1000.0,
            "outputHash": format!("{hash:016x}"), "outputBytes": bytes,
            "readBytes": io_after.ReadTransferCount - io_before.ReadTransferCount,
            "writeBytes": io_after.WriteTransferCount - io_before.WriteTransferCount,
            "initialPrivateBytes": initial.private_usage, "recordingPrivateBytes": retained.private_usage,
            "releasedPrivateBytes": after.private_usage, "peakPrivateBytes": after.peak_pagefile_usage,
            "peakWorkingSetBytes": after.peak_working_set,
        })
    );
}
