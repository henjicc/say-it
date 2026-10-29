use std::collections::BTreeSet;
use std::sync::Mutex;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum AudioOwner {
    Dictation,
    Subtitles,
    Comparison,
    AudioLab,
    Legacy,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AudioLease {
    pub(crate) owner: AudioOwner,
    pub(crate) generation: u64,
}

#[derive(Default)]
struct AudioSessionState {
    owner: Option<AudioOwner>,
    generation: u64,
    consumers: BTreeSet<&'static str>,
}

#[derive(Default)]
pub(crate) struct AudioSessionCoordinator {
    inner: Mutex<AudioSessionState>,
}

impl AudioSessionCoordinator {
    pub(crate) fn is_busy(&self) -> bool {
        self.inner
            .lock()
            .map(|state| state.owner.is_some())
            .unwrap_or(true)
    }

    /// 取得音频独占权。
    ///
    /// 每次获取都推进 generation，**同一个 owner 重入也不例外**。`validate` 本就以
    /// generation 作为鉴别依据，设计意图是「一个租约对应一次获取」；若重入时沿用旧
    /// generation，新旧租约就完全相同、无法区分，后果是过期会话的 `release` 会把
    /// 新会话的租约一并释放掉。
    ///
    /// 典型路径：开启智能处理时听写停止 → finalize 早已把 lease 从会话里 take 走，
    /// 因此后处理期间按 ESC 取消并不会释放协调器 → 用户立刻重新开始听写，拿到的是
    /// 同 generation 的别名租约 → 过期 finalize 走 cleanup 把它释放，协调器变成空闲，
    /// 而新会话仍在录音。此后开启实时字幕会成功抢到麦克风，把听写的采集掐断。
    ///
    /// 这里不选择「拒绝重入」：那会让「后处理期间取消后立刻重新听写」直接报错，
    /// 而那是完全合理的用户操作。推进 generation 既保留了重入，又让过期租约自然失效。
    pub(crate) fn acquire(&self, owner: AudioOwner) -> Result<AudioLease, String> {
        let mut state = self.inner.lock().map_err(|_| "音频会话锁失败")?;
        if let Some(current) = state.owner {
            if current != owner {
                return Err(format!("麦克风正被 {current:?} 使用"));
            }
        }
        state.generation = state.generation.wrapping_add(1).max(1);
        state.owner = Some(owner);
        // 新一次获取从零开始：旧 consumer 的租约已因 generation 推进而失效。
        state.consumers.clear();
        Ok(AudioLease {
            owner,
            generation: state.generation,
        })
    }

    pub(crate) fn attach(&self, lease: &AudioLease, consumer: &'static str) -> Result<(), String> {
        let mut state = self.inner.lock().map_err(|_| "音频会话锁失败")?;
        validate(&state, lease)?;
        state.consumers.insert(consumer);
        Ok(())
    }

    pub(crate) fn release(&self, lease: &AudioLease) -> Result<(), String> {
        let mut state = self.inner.lock().map_err(|_| "音频会话锁失败")?;
        validate(&state, lease)?;
        state.owner = None;
        state.consumers.clear();
        Ok(())
    }

    pub(crate) fn can_release_device(&self, generation: u64) -> bool {
        self.inner
            .lock()
            .map(|state| state.owner.is_none() && state.generation == generation)
            .unwrap_or(false)
    }
}

fn validate(state: &AudioSessionState, lease: &AudioLease) -> Result<(), String> {
    if state.owner != Some(lease.owner) || state.generation != lease.generation {
        return Err("音频会话租约已过期".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_conflict_and_stale_release() {
        let c = AudioSessionCoordinator::default();
        let first = c.acquire(AudioOwner::Dictation).unwrap();
        assert!(c.acquire(AudioOwner::Subtitles).is_err());
        c.release(&first).unwrap();
        let second = c.acquire(AudioOwner::Dictation).unwrap();
        assert_ne!(first.generation, second.generation);
        assert!(c.release(&first).is_err());
    }

    /// 回归：同一个 owner 重入获取也必须拿到新的 generation。
    ///
    /// 否则新旧租约完全相同，过期会话的 release 会把新会话的租约一起释放掉——
    /// 新会话仍在录音，协调器却已显示空闲，此时别的域能抢走麦克风。
    #[test]
    fn reacquiring_the_same_owner_invalidates_the_previous_lease() {
        let c = AudioSessionCoordinator::default();
        let stale = c.acquire(AudioOwner::Dictation).unwrap();
        // 模拟「后处理期间取消」：会话侧已经 take 走 lease，协调器未被释放。
        let fresh = c.acquire(AudioOwner::Dictation).unwrap();

        assert_ne!(
            stale.generation, fresh.generation,
            "重入获取必须推进 generation，否则新旧租约无法区分"
        );
        assert!(
            c.release(&stale).is_err(),
            "过期租约不得释放掉新会话的独占权"
        );
        // 新租约仍然有效，独占权还在。
        assert!(c.is_busy());
        assert!(c.attach(&fresh, "dictation").is_ok());
        c.release(&fresh).unwrap();
        assert!(!c.is_busy());
    }

    #[test]
    fn delayed_release_only_applies_to_same_idle_generation() {
        let c = AudioSessionCoordinator::default();
        let first = c.acquire(AudioOwner::Dictation).unwrap();
        c.release(&first).unwrap();
        assert!(c.can_release_device(first.generation));
        let second = c.acquire(AudioOwner::Legacy).unwrap();
        assert!(!c.can_release_device(first.generation));
        c.release(&second).unwrap();
    }

    #[test]
    fn reports_busy_without_mutating_the_active_lease() {
        let coordinator = AudioSessionCoordinator::default();
        assert!(!coordinator.is_busy());
        let lease = coordinator.acquire(AudioOwner::Subtitles).unwrap();
        assert!(coordinator.is_busy());
        coordinator.release(&lease).unwrap();
        assert!(!coordinator.is_busy());
    }
}
