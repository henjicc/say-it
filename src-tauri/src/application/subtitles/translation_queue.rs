use std::collections::{BTreeMap, VecDeque};
use tokio_util::sync::CancellationToken;

pub(super) const MAX_ACTIVE: usize = 8;
const MAX_PENDING: usize = 256;
const MAX_PENDING_BYTES: usize = 4 * 1024 * 1024;

pub(super) struct Ready<T> {
    pub seq: u64,
    pub job: T,
    pub cancellation: CancellationToken,
}

struct Pending<T> {
    seq: u64,
    bytes: usize,
    job: T,
}

pub(super) struct Queue<T> {
    pending: VecDeque<Pending<T>>,
    pending_bytes: usize,
    active: BTreeMap<u64, CancellationToken>,
}

impl<T> Default for Queue<T> {
    fn default() -> Self {
        Self {
            pending: VecDeque::new(),
            pending_bytes: 0,
            active: BTreeMap::new(),
        }
    }
}

impl<T> Queue<T> {
    pub fn enqueue(
        &mut self,
        seq: u64,
        bytes: usize,
        job: T,
        parent: &CancellationToken,
    ) -> Result<Vec<Ready<T>>, String> {
        if parent.is_cancelled() {
            return Err("字幕翻译已停止".into());
        }
        if self.active.contains_key(&seq) || self.pending.iter().any(|pending| pending.seq == seq) {
            return Err("字幕翻译任务重复".into());
        }
        // 无排队时不额外限制单次输入；请求本身继续使用供应商现有的输入限制。
        if self.active.len() < MAX_ACTIVE && self.pending.is_empty() {
            return Ok(vec![self.activate(seq, job, parent)]);
        }
        if self.pending.len() >= MAX_PENDING
            || bytes > MAX_PENDING_BYTES.saturating_sub(self.pending_bytes)
        {
            return Err("翻译服务处理过慢，等待队列已满，本段未能翻译；原文字幕仍保留".into());
        }
        self.pending_bytes += bytes;
        self.pending.push_back(Pending { seq, bytes, job });
        Ok(self.take_ready(parent))
    }

    fn activate(&mut self, seq: u64, job: T, parent: &CancellationToken) -> Ready<T> {
        let cancellation = parent.child_token();
        self.active.insert(seq, cancellation.clone());
        Ready {
            seq,
            job,
            cancellation,
        }
    }

    fn take_ready(&mut self, parent: &CancellationToken) -> Vec<Ready<T>> {
        let mut ready = Vec::new();
        if parent.is_cancelled() {
            return ready;
        }
        while self.active.len() < MAX_ACTIVE {
            let Some(pending) = self.pending.pop_front() else {
                break;
            };
            self.pending_bytes -= pending.bytes;
            ready.push(self.activate(pending.seq, pending.job, parent));
        }
        ready
    }

    pub fn finish(&mut self, seq: u64, parent: &CancellationToken) -> Vec<Ready<T>> {
        if self.active.remove(&seq).is_none() {
            return Vec::new();
        }
        self.take_ready(parent)
    }

    pub fn retain(&mut self, mut visible: impl FnMut(u64) -> bool) {
        self.pending.retain(|pending| {
            if visible(pending.seq) {
                true
            } else {
                self.pending_bytes -= pending.bytes;
                false
            }
        });
        for (&seq, cancellation) in &self.active {
            if !visible(seq) {
                cancellation.cancel();
            }
        }
        // 取消不立即释放名额；底层操作实际退出后 finish 才允许下一个任务开始。
    }
}

impl<T> Drop for Queue<T> {
    fn drop(&mut self) {
        for cancellation in self.active.values() {
            cancellation.cancel();
        }
    }
}

#[cfg(test)]
mod tests;
