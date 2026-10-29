use super::*;

fn motion(x: f64, at: Instant) -> PointerSample {
    PointerSample {
        x,
        y: 20.0,
        at,
        button_down: false,
        left_pressed: false,
        left_released: false,
        native_click_count: 0,
    }
}

#[test]
fn idle_wait_has_no_deadline_when_disabled_and_preserves_enabled_health_checks() {
    let recognizer = GestureRecognizer::default();
    let health = Instant::now() + MONITOR_HEALTH_INTERVAL;
    assert_eq!(monitor_deadline(false, &recognizer, health), None);
    assert_eq!(monitor_deadline(true, &recognizer, health), Some(health));
}

#[test]
fn dwell_deadline_moves_with_motion_and_clears_after_detection_or_reset() {
    let start = Instant::now();
    let health = start + MONITOR_HEALTH_INTERVAL;
    let mut recognizer = GestureRecognizer::default();
    for (index, x) in [0.0, 90.0, 10.0, 100.0, 20.0, 110.0]
        .into_iter()
        .enumerate()
    {
        let at = start + Duration::from_millis(index as u64 * 70);
        recognizer.push(motion(x, at));
        assert_eq!(
            recognizer.next_tick(),
            (index >= 2).then_some(at + STOP_DWELL)
        );
    }
    let deadline = monitor_deadline(true, &recognizer, health).unwrap();
    assert_eq!(
        recognizer.tick(deadline - Duration::from_micros(1), 50),
        None
    );
    assert_eq!(recognizer.tick(deadline, 50), Some((110, 20)));
    assert_eq!(recognizer.next_tick(), None);
    assert_eq!(monitor_deadline(true, &recognizer, health), Some(health));
    recognizer.reset_all();
    assert_eq!(recognizer.next_tick(), None);
}

#[test]
fn short_rejected_and_drag_sequences_do_not_leave_an_expired_deadline() {
    let start = Instant::now();
    for count in [1, 2, 5] {
        let mut recognizer = GestureRecognizer::default();
        for index in 0..count {
            recognizer.push(motion(
                index as f64 * 10.0,
                start + Duration::from_millis(index * 60),
            ));
        }
        recognizer.tick(start + Duration::from_secs(1), 50);
        assert_eq!(recognizer.next_tick(), None);
        recognizer.push(PointerSample {
            button_down: true,
            ..motion(90.0, start + Duration::from_secs(2))
        });
        assert_eq!(recognizer.next_tick(), None);
    }
}

#[test]
fn changes_and_pointer_events_wake_an_indefinite_wait_and_disconnect_exits() {
    let (sender, receiver) = channel();
    let (done, result) = channel();
    let worker = std::thread::spawn(move || {
        for _ in 0..2 {
            done.send(
                wait_for_monitor_event(&receiver, None)
                    .unwrap()
                    .map(|sample| sample.x),
            )
            .unwrap();
        }
        assert!(wait_for_monitor_event(&receiver, None).is_err());
    });
    assert!(result.recv_timeout(Duration::from_millis(40)).is_err());
    sender.send(MonitorEvent::Changed).unwrap();
    assert_eq!(result.recv_timeout(Duration::from_secs(1)).unwrap(), None);
    sender
        .send(MonitorEvent::Pointer(motion(123.0, Instant::now())))
        .unwrap();
    assert_eq!(
        result.recv_timeout(Duration::from_secs(1)).unwrap(),
        Some(123.0)
    );
    drop(sender);
    worker.join().unwrap();
}

#[test]
fn health_deadline_precedes_later_dwell_and_timeout_returns_without_a_pointer() {
    let start = Instant::now();
    let mut recognizer = GestureRecognizer::default();
    for index in 0..3 {
        recognizer.push(motion(index as f64 * 50.0, start));
    }
    assert_eq!(monitor_deadline(true, &recognizer, start), Some(start));
    let (_sender, receiver) = channel();
    assert!(wait_for_monitor_event(&receiver, Some(start))
        .unwrap()
        .is_none());
}

#[cfg(windows)]
#[test]
#[ignore = "独立鼠标识别线程空闲等待测量；不安装监听或发送鼠标事件"]
fn idle_monitor_profile() {
    use crate::performance_test_support::{memory, thread_cycles};
    let legacy = std::env::var("SAYIT_PERF_GESTURE_LEGACY").as_deref() == Ok("1");
    let enabled = std::env::var("SAYIT_PERF_GESTURE_ENABLED").as_deref() == Ok("1");
    let (sender, receiver) = channel();
    let done = std::sync::Arc::new(AtomicBool::new(false));
    let completed = done.clone();
    let producer = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(2));
        completed.store(true, Ordering::Release);
        sender.send(MonitorEvent::Changed).unwrap();
    });
    let initial = memory();
    let before = thread_cycles();
    let start = Instant::now();
    let recognizer = GestureRecognizer::default();
    let mut health = start + MONITOR_HEALTH_INTERVAL;
    let mut waits = 0;
    while !done.load(Ordering::Acquire) {
        let deadline = if legacy {
            Some(Instant::now() + Duration::from_millis(16))
        } else {
            monitor_deadline(enabled, &recognizer, health)
        };
        wait_for_monitor_event(&receiver, deadline).unwrap();
        waits += 1;
        if enabled && Instant::now() >= health {
            health = Instant::now() + MONITOR_HEALTH_INTERVAL;
        }
    }
    let cycles = thread_cycles() - before;
    let elapsed = start.elapsed().as_secs_f64() * 1000.0;
    producer.join().unwrap();
    let after = memory();
    println!(
        "PERF_RESULT {}",
        serde_json::json!({
            "scenario":"gesture-idle", "legacy":legacy, "enabled":enabled,
            "elapsedMs":elapsed, "threadCycles":cycles, "waits":waits,
            "initialPrivateBytes":initial.private_usage, "peakPrivateBytes":after.peak_pagefile_usage,
        })
    );
}
