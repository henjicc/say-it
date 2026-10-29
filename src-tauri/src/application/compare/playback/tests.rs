use super::super::{CompareCellSnapshot, CompareRuntime};
use super::*;
use std::path::PathBuf;
use std::sync::{mpsc as sync_mpsc, Arc};
use std::time::Duration;

struct AudioFile(PathBuf);
impl AudioFile {
    fn new(seconds: f32, rate: u32) -> Self {
        let path =
            std::env::temp_dir().join(format!("say-it-playback-{}.wav", uuid::Uuid::new_v4()));
        crate::audio_prep::write_test_stereo_wav(&path, seconds, rate);
        Self(path)
    }
    fn path(&self) -> &str {
        self.0.to_str().unwrap()
    }
}
impl Drop for AudioFile {
    fn drop(&mut self) {
        std::fs::remove_file(&self.0).expect("清理测试音频");
    }
}

#[test]
fn streamed_packets_match_collected_audio_and_exact_tail() {
    for rate in [8_000, 16_000, 44_100, 48_000, 96_000] {
        for seconds in [0.037, 0.2, 1.237] {
            let file = AudioFile::new(seconds, rate);
            let expected = crate::audio_prep::decode_to_mono_16k(file.path()).unwrap();
            assert_eq!(
                inspect(file.path(), || Ok(())).unwrap(),
                expected.len() as u64
            );
            let (mut rx, worker) = start(file.path().to_owned(), || Ok(()));
            let mut packets = Vec::new();
            while let Some(packet) = rx.blocking_recv() {
                packets.push(packet);
            }
            assert_eq!(
                tauri::async_runtime::block_on(worker).unwrap().unwrap(),
                expected.len() as u64
            );
            assert_eq!(packets.len(), expected.len().div_ceil(PACKET_SAMPLES));
            for (packet, reference) in packets.iter().zip(expected.chunks(PACKET_SAMPLES)) {
                assert_eq!(packet, reference);
            }
        }
    }
}

#[test]
fn stalled_receiver_bounds_prefetch_and_dropping_it_releases_decoder() {
    let file = Arc::new(AudioFile::new(5.0, 48_000));
    let (tx, rx) = mpsc::channel(PREFETCH_PACKETS);
    let (attempt_tx, attempt_rx) = sync_mpsc::channel();
    let (done_tx, done_rx) = sync_mpsc::channel();
    let producer_file = file.clone();
    let producer = std::thread::spawn(move || {
        let mut count = 0;
        let result = decode_packets(
            producer_file.path(),
            || Ok(()),
            |packet| {
                count += 1;
                attempt_tx.send(count).unwrap();
                tx.blocking_send(packet).map_err(|_| "closed".to_string())
            },
        );
        done_tx.send(result).unwrap();
    });
    for n in 1..=PREFETCH_PACKETS + 1 {
        assert_eq!(attempt_rx.recv_timeout(Duration::from_secs(2)).unwrap(), n);
    }
    assert_eq!(rx.len(), PREFETCH_PACKETS);
    assert!(done_rx.try_recv().is_err());
    drop(rx);
    assert_eq!(
        done_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
        Err("closed".into())
    );
    producer.join().unwrap();
}

#[test]
fn inspection_and_streaming_propagate_cancellation_and_invalid_input() {
    let file = AudioFile::new(1.0, 48_000);
    assert_eq!(
        inspect(file.path(), || Err("cancel".into())),
        Err("cancel".into())
    );
    let mut calls = 0;
    assert_eq!(
        decode_packets(
            file.path(),
            || Ok(()),
            |_| {
                calls += 1;
                Err("consumer stopped".into())
            }
        ),
        Err("consumer stopped".into())
    );
    assert_eq!(calls, 1);
    let mut checks = 0;
    assert_eq!(
        inspect(file.path(), || {
            checks += 1;
            if checks > 3 {
                Err("cancel mid-decode".into())
            } else {
                Ok(())
            }
        }),
        Err("cancel mid-decode".into())
    );
    std::fs::write(&file.0, b"not an audio file").unwrap();
    assert!(inspect(file.path(), || Ok(())).is_err());
    let (mut rx, worker) = start(file.path().to_owned(), || Ok(()));
    assert!(rx.blocking_recv().is_none());
    assert!(tauri::async_runtime::block_on(worker).unwrap().is_err());
}

fn runtime() -> (CompareRuntime, u64) {
    let runtime = CompareRuntime::default();
    let epoch = runtime.reset(vec![CompareCellSnapshot {
        index: 0,
        ..Default::default()
    }]);
    runtime
        .inner
        .lock()
        .unwrap()
        .sessions
        .insert("old".into(), 0);
    runtime.begin_playback(epoch, 3201).unwrap();
    (runtime, epoch)
}

#[test]
fn cancelled_playback_never_advances_or_finishes_a_new_run() {
    let (runtime, old_epoch) = runtime();
    let next = runtime.reset(vec![]);
    runtime
        .inner
        .lock()
        .unwrap()
        .sessions
        .insert("new".into(), 1);
    runtime.begin_playback(next, 4800).unwrap();
    assert!(runtime.check_playback_epoch(old_epoch).is_err());
    assert!(!runtime.begin_playback(old_epoch, 3201).unwrap());
    assert!(runtime.advance_playback(old_epoch, 3201, 3201).is_none());
    assert!(runtime.finish_playback(old_epoch, None).is_none());
    let snapshot = runtime.snapshot();
    assert_eq!(snapshot.phase, "playing");
    assert_eq!(snapshot.playback_progress.unwrap().current_ms, 0);
    assert_eq!(
        runtime.advance_playback(next, 1600, 4800).unwrap(),
        vec!["new"]
    );
}

#[test]
fn decoder_failure_closes_only_realtime_sessions_and_waits_for_file_jobs() {
    let (runtime, epoch) = runtime();
    runtime
        .inner
        .lock()
        .unwrap()
        .jobs
        .insert("file-job".into(), 1);
    let error = "decode failed".to_string();
    assert_eq!(
        runtime.finish_playback(epoch, Some(&error)).unwrap(),
        vec!["old"]
    );
    let state = runtime.inner.lock().unwrap();
    assert!(state.sessions.is_empty());
    assert_eq!(state.jobs.len(), 1);
    assert_eq!(state.phase, "finalizing");
    assert_eq!(state.cells[0].error_message, error);
    assert_eq!(state.cells[0].status, "error");
}

#[test]
fn completion_keeps_tail_progress_and_stops_when_all_consumers_have_finished() {
    let (runtime, epoch) = runtime();
    runtime.advance_playback(epoch, 3201, 3201).unwrap();
    let progress = runtime.snapshot().playback_progress.unwrap();
    assert_eq!(progress.current_ms, progress.duration_ms);
    assert_eq!(runtime.finish_playback(epoch, None).unwrap(), vec!["old"]);
    let (runtime, epoch) = self::runtime();
    runtime.inner.lock().unwrap().sessions.clear();
    assert!(runtime.advance_playback(epoch, 1600, 3201).is_none());
    assert_eq!(runtime.snapshot().phase, "idle");
}
