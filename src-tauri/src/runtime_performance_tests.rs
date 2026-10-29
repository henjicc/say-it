//! 仅本地 TCP/JSON 压力，不访问供应商、设备或应用数据。
use crate::performance_test_support::memory;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Barrier;
use tokio::task::JoinSet;

fn percentile(samples: &mut [f64], percent: usize) -> f64 {
    samples.sort_by(f64::total_cmp);
    samples[(samples.len() - 1) * percent / 100]
}

#[test]
#[ignore = "独立异步调度压力；仅本地回环网络"]
fn scheduling_profile() {
    let workers = std::env::var("SAYIT_PERF_RUNTIME_WORKERS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|&n| n > 0)
        .unwrap_or_else(|| std::thread::available_parallelism().unwrap().get());
    let initial = memory();
    let started = Instant::now();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(workers)
        .enable_all()
        .build()
        .unwrap();
    let (mut request_delays, mut timer_delays, bytes) = runtime.block_on(async {
        tokio::time::timeout(Duration::from_secs(30), async {
            const CLIENTS: usize = 8;
            const FRAMES: usize = 256;
            let body = Arc::new("识别文字".repeat(4_096));
            let expected_bytes = body.len();
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let server_body = body.clone();
            let server = tokio::spawn(async move {
                let mut connections = JoinSet::new();
                for _ in 0..CLIENTS {
                    let (mut socket, _) = listener.accept().await.unwrap();
                    socket.set_nodelay(true).unwrap();
                    let body = server_body.clone();
                    connections.spawn(async move {
                        let mut buffer = Vec::new();
                        for sequence in 0..FRAMES {
                            let len = socket.read_u32().await.unwrap() as usize;
                            assert!(len < 64 * 1024);
                            buffer.resize(len, 0);
                            socket.read_exact(&mut buffer).await.unwrap();
                            let value: serde_json::Value = serde_json::from_slice(&buffer).unwrap();
                            assert_eq!(value["sequence"].as_u64(), Some(sequence as u64));
                            assert_eq!(value["text"].as_str(), Some(body.as_str()));
                            socket.write_u64(sequence as u64).await.unwrap();
                        }
                    });
                }
                while let Some(done) = connections.join_next().await {
                    done.unwrap();
                }
            });
            let start = Arc::new(Barrier::new(CLIENTS + 2));
            let probe_start = start.clone();
            let probe = tokio::spawn(async move {
                probe_start.wait().await;
                let mut clock = tokio::time::interval(Duration::from_millis(4));
                clock.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                let mut delays = Vec::new();
                for _ in 0..512 {
                    let deadline = clock.tick().await;
                    delays.push(deadline.elapsed().as_secs_f64() * 1_000.0);
                }
                delays
            });
            let mut clients = JoinSet::new();
            for _ in 0..CLIENTS {
                let body = body.clone();
                let start = start.clone();
                clients.spawn(async move {
                    let mut socket = TcpStream::connect(address).await.unwrap();
                    socket.set_nodelay(true).unwrap();
                    start.wait().await;
                    let mut clock = tokio::time::interval(Duration::from_millis(8));
                    clock.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                    let mut delays = Vec::new();
                    for sequence in 0..FRAMES {
                        clock.tick().await;
                        let sent = Instant::now();
                        let payload = serde_json::to_vec(&serde_json::json!({
                            "sequence":sequence,"text":body.as_str(),
                        }))
                        .unwrap();
                        socket.write_u32(payload.len() as u32).await.unwrap();
                        socket.write_all(&payload).await.unwrap();
                        assert_eq!(socket.read_u64().await.unwrap(), sequence as u64);
                        delays.push(sent.elapsed().as_secs_f64() * 1_000.0);
                    }
                    delays
                });
            }
            start.wait().await;
            let mut delays = Vec::new();
            while let Some(done) = clients.join_next().await {
                delays.extend(done.unwrap());
            }
            server.await.unwrap();
            assert_eq!(delays.len(), CLIENTS * FRAMES);
            (
                delays,
                probe.await.unwrap(),
                CLIENTS * FRAMES * expected_bytes,
            )
        })
        .await
        .expect("本地调度压力在 30 秒内未完成")
    });
    let elapsed = started.elapsed().as_secs_f64() * 1_000.0;
    let after = memory();
    println!(
        "PERF_RESULT {}",
        serde_json::json!({
            "scenario":"runtime-scheduling", "workers":workers, "requests":request_delays.len(),
            "verifiedTextBytes":bytes, "elapsedMs":elapsed,
            "requestP95Ms":percentile(&mut request_delays,95),
            "requestP99Ms":percentile(&mut request_delays,99),
            "timerP95Ms":percentile(&mut timer_delays,95),
            "timerP99Ms":percentile(&mut timer_delays,99),
            "initialPrivateBytes":initial.private_usage,"finalPrivateBytes":after.private_usage,
            "peakPrivateBytes":after.peak_pagefile_usage,
        })
    );
}
