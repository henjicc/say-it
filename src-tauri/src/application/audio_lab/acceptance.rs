//! 仅替代设备输入；处理、快照、试听文件仍走正式应用命令。
use super::*;

impl AudioLabRuntime {
    pub(crate) fn acceptance_seed(&self, seconds: usize) -> Result<(), String> {
        if !(1..=1800).contains(&seconds) {
            return Err("验收素材时长越界".into());
        }
        let epoch = self.begin(48_000)?;
        let chunk: Vec<f32> = (0..480)
            .map(|index| (index as f32 / 480.0 - 0.5) * 0.2)
            .collect();
        for _ in 0..seconds * 100 {
            self.append_for(epoch, &chunk)?;
        }
        self.stop()
    }

    pub(crate) fn acceptance_processed_hash(&self, seconds: usize) -> Result<String, String> {
        let state = self.state.lock().map_err(|_| "验收状态锁失败")?;
        if state.processed.len() != seconds * 48_000 {
            return Err("处理后样本数量不匹配".into());
        }
        let mut hash = 0xcbf29ce484222325_u64;
        state
            .processed
            .visit(|samples| {
                for value in samples {
                    hash = (hash ^ u64::from(value.to_bits())).wrapping_mul(0x100000001b3);
                }
                Ok(())
            })
            .map_err(|error| error.to_string())?;
        Ok(format!("{hash:016x}"))
    }
}
