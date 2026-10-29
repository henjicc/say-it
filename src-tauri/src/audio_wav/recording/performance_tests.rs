use super::*;
use crate::performance_test_support::memory;
use std::io::Read;
use std::time::Instant;

#[test]
#[ignore = "独立录音存储测量：合成输入，不打开麦克风或调用识别服务"]
fn recording_storage_profile() {
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
    let initial = memory();
    let io_before = io();
    let started = Instant::now();
    let retained;
    let stop_started;
    let file;
    if legacy {
        let mut raw = Vec::new();
        for offset in (0..seconds * 48_000).step_by(input.len()) {
            raw.extend_from_slice(&input[..(seconds * 48_000 - offset).min(input.len())]);
        }
        retained = memory();
        stop_started = Instant::now();
        let path = std::env::temp_dir().join(format!(
            "say-it-recording-perf-{}.wav",
            uuid::Uuid::new_v4()
        ));
        super::super::write_mono_pcm16(&path, &raw, 48_000, Quantization::Round).unwrap();
        file = RecordedWav {
            path: Some(path),
            samples: raw.len(),
        };
    } else {
        let mut recording = WavRecording::new(48_000, Quantization::Round).unwrap();
        for offset in (0..seconds * 48_000).step_by(input.len()) {
            recording
                .append(&input[..(seconds * 48_000 - offset).min(input.len())])
                .unwrap();
        }
        retained = memory();
        stop_started = Instant::now();
        file = recording.finish().unwrap();
    }
    let elapsed = started.elapsed();
    let stop_elapsed = stop_started.elapsed();
    let io_after = io();
    let after = memory();
    let mut reader = File::open(file.path()).unwrap();
    let bytes = reader.metadata().unwrap().len();
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
    assert_eq!(bytes, 44 + seconds as u64 * 48_000 * 2);
    assert_eq!(file.samples, seconds * 48_000);
    drop(reader);
    drop(file);
    println!(
        "PERF_RESULT {}",
        serde_json::json!({
            "scenario": "dictation-recording-storage", "legacy": legacy, "seconds": seconds,
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
