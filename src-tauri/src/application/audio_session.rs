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
    pub(crate) fn acquire(&self, owner: AudioOwner) -> Result<AudioLease, String> {
        let mut state = self.inner.lock().map_err(|_| "音频会话锁失败")?;
        if let Some(current) = state.owner {
            if current != owner {
                return Err(format!("麦克风正被 {current:?} 使用"));
            }
        } else {
            state.generation = state.generation.wrapping_add(1).max(1);
            state.owner = Some(owner);
        }
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
}
