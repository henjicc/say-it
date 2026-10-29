use super::super::wait_for_stop;
use super::*;
use crate::cancellation::CancellationFlag;
use crate::performance_test_support::{memory, thread_cycles};
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

#[test]
#[ignore = "独立宿主网络等待测量；不访问供应商或设备"]
fn network_wait_profile() {
    let legacy = std::env::var("SAYIT_PERF_HOST_WAIT_LEGACY").as_deref() == Ok("1");
    let cancel = std::env::var("SAYIT_PERF_HOST_WAIT_CANCEL").as_deref() == Ok("1");
    // 限定同一线程，cycles 不混入其他 Tokio worker 的无关任务。
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    let initial = memory();
    let cycles = thread_cycles();
    let started = Instant::now();
    let mut polls = 0;
    let mut latency = Vec::new();
    runtime.block_on(async {
        for index in 0..if cancel { 64 } else { 1 } {
            let flag = Arc::new(CancellationFlag::default());
            let deadline = Arc::new(Deadline::new(Instant::now() + Duration::from_secs(2)));
            let producer = if cancel {
                let flag = flag.clone();
                Some(std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis((index % 7 + 1) as u64));
                    let sent = Instant::now();
                    flag.store(true, std::sync::atomic::Ordering::Release);
                    sent
                }))
            } else {
                None
            };
            let waiting = async {
                if legacy {
                    // 冻结旧宿主实现；其取消检查是每 25ms 一次。
                    while !flag.load(std::sync::atomic::Ordering::Relaxed) && !deadline.expired() {
                        tokio::time::sleep(Duration::from_millis(25)).await;
                    }
                } else {
                    wait_for_stop(flag, deadline).await;
                }
            };
            let mut waiting = std::pin::pin!(waiting);
            std::future::poll_fn(|context| {
                polls += 1;
                waiting.as_mut().poll(context)
            })
            .await;
            let received = Instant::now();
            if let Some(producer) = producer {
                latency.push(
                    received
                        .duration_since(producer.join().unwrap())
                        .as_secs_f64()
                        * 1000.0,
                );
            }
        }
    });
    let cycles = thread_cycles() - cycles;
    let elapsed = started.elapsed().as_secs_f64() * 1000.0;
    latency.sort_by(f64::total_cmp);
    let after = memory();
    println!(
        "PERF_RESULT {}",
        serde_json::json!({
            "scenario":"host-network-wait", "legacy":legacy, "cancel":cancel,
            "elapsedMs":elapsed, "threadCycles":cycles, "futurePolls":polls,
            "cancelP50Ms":latency.get(32), "cancelP95Ms":latency.get(60), "cancelMaxMs":latency.last(),
            "initialPrivateBytes":initial.private_usage, "peakPrivateBytes":after.peak_pagefile_usage,
        })
    );
}
