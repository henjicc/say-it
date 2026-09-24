use super::*;

fn fill_active(queue: &mut Queue<u64>, parent: &CancellationToken) -> Vec<Ready<u64>> {
    (1..=MAX_ACTIVE as u64)
        .flat_map(|seq| queue.enqueue(seq, 1, seq, parent).unwrap())
        .collect()
}

#[test]
fn active_and_waiting_counts_are_bounded_and_fifo_work_is_preserved() {
    let parent = CancellationToken::new();
    let mut queue = Queue::default();
    let mut ready: VecDeque<_> = fill_active(&mut queue, &parent).into();
    for seq in MAX_ACTIVE as u64 + 1..=(MAX_ACTIVE + MAX_PENDING) as u64 {
        assert!(queue.enqueue(seq, 1, seq, &parent).unwrap().is_empty());
    }
    assert!(queue
        .enqueue(999, 1, 999, &parent)
        .err()
        .unwrap()
        .contains("队列已满"));
    assert_eq!(queue.active.len(), MAX_ACTIVE);
    assert_eq!(queue.pending.len(), MAX_PENDING);
    let mut completed = Vec::new();
    while let Some(task) = ready.pop_front() {
        assert_eq!(task.seq, task.job);
        completed.push(task.seq);
        ready.extend(queue.finish(task.seq, &parent));
        assert!(queue.active.len() <= MAX_ACTIVE);
    }
    assert_eq!(
        completed,
        (1..=(MAX_ACTIVE + MAX_PENDING) as u64).collect::<Vec<_>>()
    );
    assert!(queue.active.is_empty() && queue.pending.is_empty());
    assert_eq!(queue.pending_bytes, 0);
}

#[test]
fn byte_limit_is_independent_of_count_and_recovers_after_completion() {
    let parent = CancellationToken::new();
    let mut queue = Queue::default();
    fill_active(&mut queue, &parent);
    queue.enqueue(9, MAX_PENDING_BYTES, 9, &parent).unwrap();
    assert!(queue.enqueue(10, 1, 10, &parent).is_err());
    assert_eq!(queue.pending_bytes, MAX_PENDING_BYTES);
    assert_eq!(queue.finish(1, &parent)[0].seq, 9);
    assert_eq!(queue.pending_bytes, 0);
    assert!(queue.enqueue(10, 1, 10, &parent).unwrap().is_empty());
}

#[test]
fn obsolete_work_is_cancelled_but_keeps_its_slot_until_it_exits() {
    let parent = CancellationToken::new();
    let mut queue = Queue::default();
    let active = fill_active(&mut queue, &parent);
    queue.enqueue(9, 3, 9, &parent).unwrap();
    queue.enqueue(10, 5, 10, &parent).unwrap();
    queue.retain(|seq| seq != 1 && seq != 9);
    assert!(active[0].cancellation.is_cancelled());
    assert!(!active[1].cancellation.is_cancelled());
    assert!(!parent.is_cancelled());
    assert_eq!(queue.active.len(), MAX_ACTIVE);
    assert_eq!(queue.pending_bytes, 5);
    assert_eq!(queue.finish(1, &parent)[0].seq, 10);
    assert!(
        queue.finish(1, &parent).is_empty(),
        "重复退出不能释放别的名额"
    );
    queue.enqueue(11, 7, 11, &parent).unwrap();
    queue.retain(|_| false);
    assert!(active.iter().all(|task| task.cancellation.is_cancelled()));
    assert!(queue.pending.is_empty());
    assert_eq!(queue.pending_bytes, 0);
    assert!(!parent.is_cancelled(), "关闭翻译不应取消整段字幕会话");
}

#[test]
fn stop_prevents_pending_work_from_starting_and_dropping_cancels_active_work() {
    let parent = CancellationToken::new();
    let mut queue = Queue::default();
    let active = fill_active(&mut queue, &parent);
    queue.enqueue(9, 1, 9, &parent).unwrap();
    parent.cancel();
    assert!(active.iter().all(|task| task.cancellation.is_cancelled()));
    assert!(queue.finish(1, &parent).is_empty());
    assert!(queue.enqueue(10, 1, 10, &parent).is_err());
    let next_parent = CancellationToken::new();
    let mut next = Queue::default();
    let task = next.enqueue(1, 1, 1, &next_parent).unwrap().remove(0);
    drop(queue);
    assert!(!task.cancellation.is_cancelled());
    drop(next);
    assert!(task.cancellation.is_cancelled());
    assert!(!next_parent.is_cancelled());
}

#[test]
fn discarding_pending_work_releases_its_payload_without_starting_it() {
    use std::sync::Arc;
    let parent = CancellationToken::new();
    let mut queue = Queue::default();
    for seq in 0..MAX_ACTIVE as u64 {
        queue.enqueue(seq, 1, Arc::new(vec![0u8]), &parent).unwrap();
    }
    let payload = Arc::new(vec![7u8; 1024]);
    let weak = Arc::downgrade(&payload);
    queue.enqueue(100, 1024, payload, &parent).unwrap();
    assert!(weak.upgrade().is_some());
    queue.retain(|seq| seq != 100);
    assert!(weak.upgrade().is_none());
    assert_eq!(queue.pending_bytes, 0);
}

#[tokio::test]
async fn slow_local_operations_never_exceed_limit_and_all_results_complete() {
    use futures_util::{stream::FuturesUnordered, StreamExt};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    let parent = CancellationToken::new();
    let mut queue = Queue::default();
    let running = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let execute = |task: Ready<String>| {
        let running = running.clone();
        let peak = peak.clone();
        async move {
            let count = running.fetch_add(1, Ordering::SeqCst) + 1;
            peak.fetch_max(count, Ordering::SeqCst);
            tokio::time::sleep(std::time::Duration::from_millis(2)).await;
            running.fetch_sub(1, Ordering::SeqCst);
            (task.seq, task.job)
        }
    };
    let mut futures = FuturesUnordered::new();
    for seq in 0..264u64 {
        for task in queue
            .enqueue(seq, 32, format!("译文 {seq}"), &parent)
            .unwrap()
        {
            futures.push(execute(task));
        }
    }
    let mut results = BTreeMap::new();
    while let Some((seq, text)) = futures.next().await {
        results.insert(seq, text);
        for task in queue.finish(seq, &parent) {
            futures.push(execute(task));
        }
    }
    assert_eq!(peak.load(Ordering::SeqCst), MAX_ACTIVE);
    assert_eq!(results.len(), 264);
    for seq in 0..264u64 {
        assert_eq!(results[&seq], format!("译文 {seq}"));
    }
    assert_eq!(running.load(Ordering::SeqCst), 0);
    assert!(queue.active.is_empty() && queue.pending.is_empty());
}
