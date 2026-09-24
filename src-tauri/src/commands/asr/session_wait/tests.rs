use super::*;
use crate::state::AsrStreamHandle;
use std::time::Duration;

#[test]
fn quiet_session_sleeps_until_the_actual_deadline() {
    let (_handle, mut receiver) = AsrStreamHandle::channel();
    let host = Notify::new();
    let waiter = SessionWait::new();
    let end = Instant::now() + Duration::from_millis(120);
    assert!(matches!(
        waiter.wait(&mut receiver, &host, Some(end)),
        SessionWake::Deadline
    ));
    assert!(Instant::now() >= end);
    assert!(
        waiter.polls.get() <= 4,
        "quiet polls={}",
        waiter.polls.get()
    );
}

#[test]
fn expired_finish_deadline_does_not_hide_stop_or_input_failure() {
    for failed in [false, true] {
        let (handle, mut receiver) = AsrStreamHandle::channel();
        if failed {
            handle.tx.fail("device failed".into());
        } else {
            handle.stop();
        }
        let wake = SessionWait::new().wait(&mut receiver, &Notify::new(), Some(Instant::now()));
        match (failed, wake) {
            (false, SessionWake::Input(Some(AsrStreamInput::Stop))) => {}
            (true, SessionWake::Input(Some(AsrStreamInput::Failed(error)))) => {
                assert_eq!(error, "device failed")
            }
            _ => panic!("deadline hid the terminal input"),
        }
    }
}

#[test]
fn queued_and_racing_host_events_are_never_lost() {
    let (_handle, mut receiver) = AsrStreamHandle::channel();
    let host = Arc::new(Notify::new());
    let waiter = SessionWait::new();
    for queued in [true, false] {
        for _ in 0..64 {
            let notify = host.clone();
            if queued {
                notify.notify_one();
            }
            let worker = std::thread::spawn(move || {
                if !queued {
                    notify.notify_one();
                }
            });
            assert!(matches!(
                waiter.wait(
                    &mut receiver,
                    &host,
                    Some(Instant::now() + Duration::from_secs(2))
                ),
                SessionWake::HostEvents
            ));
            worker.join().unwrap();
        }
    }
}

#[test]
fn idle_audio_failure_stop_disconnect_and_namespace_cancel_wake_the_waiter() {
    for mode in 0..5 {
        let (handle, mut receiver) = AsrStreamHandle::channel();
        let host = Arc::new(Notify::new());
        let cancel = receiver.cancellation_flag();
        let notify = host.clone();
        let producer = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(15));
            match mode {
                0 => {
                    handle
                        .tx
                        .send(AsrStreamInput::RawF32(vec![0.25, -0.0]))
                        .unwrap();
                }
                1 => handle.tx.fail("test failure".into()),
                2 => handle.stop(),
                3 => {} // 最后一个发送者被释放。
                4 => {
                    cancel.store(true, std::sync::atomic::Ordering::Release);
                    notify.notify_one();
                    // 保留发送者，证明单独的宿主取消通知足以唤醒。
                    std::thread::sleep(Duration::from_millis(50));
                }
                _ => unreachable!(),
            }
        });
        let waiter = SessionWait::new();
        let end = Some(Instant::now() + Duration::from_secs(2));
        let mut wake = waiter.wait(&mut receiver, &host, end);
        if matches!(wake, SessionWake::HostEvents) {
            wake = waiter.wait(&mut receiver, &host, end);
        }
        match (mode, wake) {
            (0, SessionWake::Input(Some(AsrStreamInput::RawF32(samples)))) => {
                assert_eq!(samples.len(), 2);
                assert_eq!(samples[1].to_bits(), (-0.0f32).to_bits());
            }
            (1, SessionWake::Input(Some(AsrStreamInput::Failed(error)))) => {
                assert_eq!(error, "test failure")
            }
            (2 | 4, SessionWake::Input(Some(AsrStreamInput::Stop)))
            | (3, SessionWake::Input(None)) => {}
            _ => panic!("wrong wake reason for {mode}"),
        }
        producer.join().unwrap();
    }
}

#[test]
fn host_notifications_do_not_drop_spilled_audio_or_finish_tail() {
    let (handle, mut receiver) = AsrStreamHandle::channel();
    for i in 0..1024 {
        handle
            .tx
            .send(AsrStreamInput::RawF32(vec![i as f32; 4096]))
            .unwrap();
    }
    handle
        .tx
        .send(AsrStreamInput::RawF32(vec![-0.75; 17]))
        .unwrap();
    handle.tx.send(AsrStreamInput::Finish).unwrap();
    let host = Notify::new();
    let waiter = SessionWait::new();
    let mut index = 0;
    let end = Some(Instant::now() + Duration::from_secs(5));
    loop {
        host.notify_one();
        match waiter.wait(&mut receiver, &host, end) {
            SessionWake::Input(Some(AsrStreamInput::RawF32(samples))) => {
                if index < 1024 {
                    assert_eq!(samples, vec![index as f32; 4096]);
                } else {
                    assert_eq!(samples, [-0.75; 17]);
                }
                index += 1;
            }
            SessionWake::Input(Some(AsrStreamInput::Finish)) => break,
            SessionWake::HostEvents => std::thread::yield_now(),
            _ => panic!("lost packet or timeout"),
        }
    }
    assert_eq!(index, 1025);
}

#[cfg(windows)]
#[test]
#[ignore = "独立 JS 会话等待测量；不加载模型或访问服务"]
fn idle_session_profile() {
    use windows::Win32::System::Threading::GetCurrentThread;
    #[link(name = "kernel32")]
    extern "system" {
        fn QueryThreadCycleTime(
            thread: windows::Win32::Foundation::HANDLE,
            cycles: *mut u64,
        ) -> i32;
    }
    let legacy = std::env::var("SAYIT_PERF_SESSION_POLL_LEGACY").as_deref() == Ok("1");
    let result = crate::providers::plugin_runtime::spawn_js_worker("wait-profile", move || {
        let (_handle, mut receiver) = AsrStreamHandle::channel();
        let host = Notify::new();
        let waiter = SessionWait::new();
        let mut cycles_before = 0;
        unsafe {
            assert_ne!(
                QueryThreadCycleTime(GetCurrentThread(), &mut cycles_before),
                0
            );
        }
        let started = Instant::now();
        let deadline = started + Duration::from_secs(2);
        let mut iterations = 0;
        loop {
            iterations += 1;
            if legacy {
                assert!(receiver.try_recv().is_err());
                std::thread::sleep(Duration::from_millis(10));
            } else {
                assert!(matches!(
                    waiter.wait(&mut receiver, &host, Some(deadline)),
                    SessionWake::Deadline
                ));
            }
            if Instant::now() >= deadline {
                break;
            }
        }
        let mut cycles_after = 0;
        unsafe {
            assert_ne!(
                QueryThreadCycleTime(GetCurrentThread(), &mut cycles_after),
                0
            );
        }
        (
            iterations,
            waiter.polls.get(),
            started.elapsed().as_secs_f64() * 1000.0,
            cycles_after - cycles_before,
        )
    })
    .unwrap()
    .blocking_recv()
    .unwrap();
    let memory = crate::performance_test_support::memory();
    println!(
        "PERF_RESULT {}",
        serde_json::json!({
            "scenario":"asr-idle-session", "legacy":legacy, "elapsedMs":result.2,
        "sessionIterations":result.0, "channelPolls":result.1,
        "threadCycles":result.3,
            "peakPrivateBytes":memory.peak_pagefile_usage,
        })
    );
}

#[cfg(windows)]
#[test]
#[ignore = "独立音频/宿主消息唤醒延迟测量；不访问设备或服务"]
fn session_notification_latency_profile() {
    use futures_util::FutureExt;
    let legacy = std::env::var("SAYIT_PERF_SESSION_POLL_LEGACY").as_deref() == Ok("1");
    let result = crate::providers::plugin_runtime::spawn_js_worker("wake-profile", move || {
        let (handle, mut receiver) = AsrStreamHandle::channel();
        let host = Arc::new(Notify::new());
        let notify = host.clone();
        let (sent_tx, sent_rx) = std::sync::mpsc::sync_channel(1);
        let (ack_tx, ack_rx) = std::sync::mpsc::sync_channel(1);
        let producer = std::thread::spawn(move || {
            for index in 0..120 {
                std::thread::sleep(Duration::from_millis((index * 7 % 11) as u64));
                sent_tx.send(Instant::now()).unwrap();
                if index % 2 == 0 {
                    handle
                        .tx
                        .send(AsrStreamInput::RawF32(vec![index as f32]))
                        .unwrap();
                } else {
                    notify.notify_one();
                }
                ack_rx.recv_timeout(Duration::from_secs(2)).unwrap();
            }
        });
        let waiter = SessionWait::new();
        let started = Instant::now();
        let mut audio = Vec::new();
        let mut events = Vec::new();
        for index in 0..120 {
            let wake = if legacy {
                loop {
                    if let Ok(input) = receiver.try_recv() {
                        break SessionWake::Input(Some(input));
                    }
                    std::thread::sleep(Duration::from_millis(10));
                    if host.notified().now_or_never().is_some() {
                        break SessionWake::HostEvents;
                    }
                    assert!(started.elapsed() < Duration::from_secs(5));
                }
            } else {
                waiter.wait(
                    &mut receiver,
                    &host,
                    Some(Instant::now() + Duration::from_secs(2)),
                )
            };
            let arrived = Instant::now();
            let elapsed = arrived
                .duration_since(sent_rx.recv_timeout(Duration::from_secs(2)).unwrap())
                .as_secs_f64()
                * 1000.0;
            if index % 2 == 0 {
                let SessionWake::Input(Some(AsrStreamInput::RawF32(samples))) = wake else {
                    panic!("missing audio");
                };
                assert_eq!(samples, [index as f32]);
                audio.push(elapsed);
            } else {
                assert!(matches!(wake, SessionWake::HostEvents));
                events.push(elapsed);
            }
            ack_tx.send(()).unwrap();
        }
        producer.join().unwrap();
        audio.sort_by(f64::total_cmp);
        events.sort_by(f64::total_cmp);
        (audio, events, started.elapsed().as_secs_f64() * 1000.0)
    })
    .unwrap()
    .blocking_recv()
    .unwrap();
    let memory = crate::performance_test_support::memory();
    println!(
        "PERF_RESULT {}",
        serde_json::json!({
            "scenario":"asr-session-notification-latency", "legacy":legacy, "elapsedMs":result.2,
            "audioP50Ms":result.0[30], "audioP95Ms":result.0[57], "audioMaxMs":result.0[59],
            "hostP50Ms":result.1[30], "hostP95Ms":result.1[57], "hostMaxMs":result.1[59],
            "peakPrivateBytes":memory.peak_pagefile_usage, "audioPackets":60, "hostEvents":60,
        })
    );
}
