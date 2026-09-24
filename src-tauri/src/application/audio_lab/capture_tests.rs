use super::*;

#[test]
fn stopping_waits_for_delayed_tail_and_keeps_every_sample() {
    tauri::async_runtime::block_on(async {
        let runtime = AudioLabRuntime::default();
        let epoch = runtime.begin(48_000).unwrap();
        runtime.append_for(epoch, &[0.25; 4096]).unwrap();
        let (done, drain) = tokio::sync::oneshot::channel();
        runtime.register_drain(epoch, drain).unwrap();
        runtime.request_stop().unwrap();
        let waiting = runtime.drain_capture();
        tokio::pin!(waiting);
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(10), &mut waiting)
                .await
                .is_err()
        );
        assert!(runtime.is_recording().unwrap());
        assert!(runtime.begin(16_000).is_err());
        runtime.append_for(epoch, &[-0.5; 17]).unwrap();
        done.send(runtime.finish_input(epoch)).unwrap();
        waiting.await.unwrap();
        runtime.stop().unwrap();
        let state = runtime.state.lock().unwrap();
        assert!(!state.recording);
        assert_eq!(&state.raw[..4096], &[0.25; 4096]);
        assert_eq!(&state.raw[4096..], &[-0.5; 17]);
    });
}

#[test]
fn capture_errors_and_missing_consumers_fail_the_take() {
    for failure in 0..3 {
        tauri::async_runtime::block_on(async {
            let runtime = AudioLabRuntime::default();
            let epoch = runtime.begin(48_000).unwrap();
            runtime.append_for(epoch, &[0.25]).unwrap();
            if failure != 2 {
                let (done, drain) = tokio::sync::oneshot::channel();
                runtime.register_drain(epoch, drain).unwrap();
                if failure == 0 {
                    done.send(Err("暂存写入失败".into())).unwrap();
                }
            }
            runtime.request_stop().unwrap();
            let error = runtime.drain_capture().await.unwrap_err();
            let snapshot = runtime.snapshot().unwrap();
            assert!(!snapshot.recording);
            assert_eq!(snapshot.error.as_deref(), Some(error.as_str()));
            assert_eq!(runtime.domain_snapshot().state, DomainRunState::Failed);
        });
    }
}

#[test]
fn old_capture_cannot_append_or_fail_a_new_take() {
    let runtime = AudioLabRuntime::default();
    let old = runtime.begin(48_000).unwrap();
    runtime.abort();
    let current = runtime.begin(16_000).unwrap();
    assert_ne!(old, current);
    runtime.append_for(current, &[0.5]).unwrap();
    assert!(runtime.append_for(old, &[-0.5]).is_err());
    assert!(!runtime.fail_for(old, "旧设备错误".into()));
    assert!(runtime.finish_input(old).is_err());
    assert!(runtime.is_recording().unwrap());
    let state = runtime.state.lock().unwrap();
    assert_eq!(state.raw, vec![0.5]);
    assert!(state.error.is_none());
}

#[test]
fn unexpected_end_is_not_a_successful_stop() {
    let runtime = AudioLabRuntime::default();
    let epoch = runtime.begin(48_000).unwrap();
    assert_eq!(
        runtime.finish_input(epoch).unwrap_err(),
        "音频采集已意外停止"
    );
    runtime.request_stop().unwrap();
    assert!(runtime.finish_input(epoch).is_ok());
}
