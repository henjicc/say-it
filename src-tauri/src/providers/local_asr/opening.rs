//! 正在建立的本地识别会话。识别模型在听写的关键路径上，其余本地模型的预载要等它们建好再读盘：
//! 两个大模型同时从机械硬盘读取会来回寻道，谁都加载不完。

use std::sync::OnceLock;

use tokio::sync::watch;

fn count() -> &'static watch::Sender<usize> {
    static COUNT: OnceLock<watch::Sender<usize>> = OnceLock::new();
    COUNT.get_or_init(|| watch::channel(0).0)
}

/// 从决定开会话起持有，到会话建好或建立失败时丢弃。
pub(crate) struct Opening(());

pub(crate) fn begin() -> Opening {
    count().send_modify(|n| *n += 1);
    Opening(())
}

impl Drop for Opening {
    fn drop(&mut self) {
        count().send_modify(|n| *n -= 1);
    }
}

/// 等当前所有正在建立的本地识别会话建好（或失败）。
pub(crate) async fn settled() {
    let mut rx = count().subscribe();
    // 发送端是静态的，不会关闭。
    let _ = rx.wait_for(|n| *n == 0).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::FutureExt;

    #[tokio::test]
    async fn waits_until_every_opening_session_settles() {
        let first = begin();
        let second = begin();
        let waiting = settled();
        tokio::pin!(waiting);
        assert!((&mut waiting).now_or_never().is_none());
        drop(first);
        assert!((&mut waiting).now_or_never().is_none());
        drop(second);
        assert!(waiting.now_or_never().is_some());
    }
}
