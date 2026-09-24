use super::*;
use crate::state::AsrStreamInput;
use std::sync::atomic::{AtomicUsize, Ordering};

fn streams() -> Arc<Mutex<HashMap<String, AsrStreamHandle>>> {
    Arc::new(Mutex::new(HashMap::new()))
}

#[test]
fn initialization_and_immediate_events_wait_for_owner_registration() {
    let streams = streams();
    let (handle, _rx) = AsrStreamHandle::channel();
    streams.lock().unwrap().insert("s".into(), handle);
    let owner = Arc::new(Mutex::new(None::<String>));
    let calls = Arc::new(AtomicUsize::new(0));
    let output = Arc::new(Mutex::new(None));
    let captured = (owner.clone(), calls.clone(), output.clone());
    let prepared = PreparedAsrStream::new("s".into(), streams.clone(), move || {
        captured.1.fetch_add(1, Ordering::SeqCst);
        // 模拟初始化立即失败/立即完成：连异步调度的一次让出都没有。
        assert_eq!(captured.0.lock().unwrap().as_deref(), Some("s"));
        *captured.2.lock().unwrap() = Some("initialization completed");
        Ok(())
    });
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    *owner.lock().unwrap() = Some(prepared.session_id.clone());
    prepared.start().unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(*output.lock().unwrap(), Some("initialization completed"));
    assert!(
        streams.lock().unwrap().contains_key("s"),
        "成功启动由会话负责清理"
    );
}

#[test]
fn abandoning_preparation_releases_audio_and_captured_resources_without_initializing() {
    let streams = streams();
    let calls = Arc::new(AtomicUsize::new(0));
    for _ in 0..1_000 {
        let (handle, rx) = AsrStreamHandle::channel();
        let sender = handle.tx.clone();
        streams.lock().unwrap().insert("s".into(), handle);
        sender
            .send(AsrStreamInput::RawF32(vec![0.5; 4096]))
            .unwrap();
        let resource = Arc::new(vec![0u8; 4096]);
        let weak = Arc::downgrade(&resource);
        let calls = calls.clone();
        let prepared = PreparedAsrStream::new("s".into(), streams.clone(), move || {
            let _owned = (resource, rx);
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        });
        drop(prepared);
        assert!(streams.lock().unwrap().is_empty());
        assert!(sender.is_closed());
        assert!(weak.upgrade().is_none());
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn launch_failure_returns_error_and_cancels_only_its_own_audio_entry() {
    let streams = streams();
    let (failed, failed_rx) = AsrStreamHandle::channel();
    let (other, other_rx) = AsrStreamHandle::channel();
    streams
        .lock()
        .unwrap()
        .extend([("failed".into(), failed), ("other".into(), other)]);
    let prepared = PreparedAsrStream::new("failed".into(), streams.clone(), move || {
        Err("模拟线程创建失败".into())
    });
    assert_eq!(prepared.start().unwrap_err(), "模拟线程创建失败");
    assert!(failed_rx.is_cancelled());
    assert!(!other_rx.is_cancelled());
    assert_eq!(streams.lock().unwrap().len(), 1);
}

#[test]
fn cancellation_before_activation_never_runs_the_initializer() {
    for remove in [false, true] {
        let streams = streams();
        let (handle, _rx) = AsrStreamHandle::channel();
        streams.lock().unwrap().insert("s".into(), handle.clone());
        let prepared =
            PreparedAsrStream::new("s".into(), streams.clone(), || panic!("取消后不能初始化"));
        handle.stop();
        if remove {
            streams.lock().unwrap().remove("s");
        }
        assert_eq!(prepared.start().unwrap_err(), "识别启动已取消");
        assert!(streams.lock().unwrap().is_empty());
    }
}

#[test]
fn audio_attached_before_activation_keeps_packet_and_finish_order() {
    let streams = streams();
    let (handle, mut rx) = AsrStreamHandle::channel();
    let sender = handle.tx.clone();
    streams.lock().unwrap().insert("s".into(), handle);
    let prepared = PreparedAsrStream::new("s".into(), streams.clone(), move || {
        for value in [0.1, 0.2, 0.3] {
            match rx.blocking_recv().unwrap() {
                AsrStreamInput::RawF32(samples) => assert_eq!(samples, vec![value; 4096]),
                _ => panic!("audio order changed"),
            }
        }
        assert!(matches!(rx.blocking_recv(), Some(AsrStreamInput::Finish)));
        Ok(())
    });
    for value in [0.1, 0.2, 0.3] {
        sender
            .send(AsrStreamInput::RawF32(vec![value; 4096]))
            .unwrap();
    }
    sender.send(AsrStreamInput::Finish).unwrap();
    prepared.start().unwrap();
}
