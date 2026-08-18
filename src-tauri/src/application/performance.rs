use serde::Serialize;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

const MAX_SAMPLES: usize = 256;
static SAMPLES: OnceLock<Mutex<HashMap<&'static str, Vec<u64>>>> = OnceLock::new();

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PerformanceMetric {
    name: String,
    samples: usize,
    p50_ms: u64,
    p95_ms: u64,
    max_ms: u64,
}

pub(crate) fn record(name: &'static str, elapsed_ms: u64) {
    let Ok(mut samples) = SAMPLES.get_or_init(|| Mutex::new(HashMap::new())).lock() else {
        return;
    };
    let values = samples.entry(name).or_default();
    if values.len() == MAX_SAMPLES {
        values.remove(0);
    }
    values.push(elapsed_ms);
}

fn percentile(values: &[u64], percentile: usize) -> u64 {
    if values.is_empty() {
        return 0;
    }
    let index = ((values.len() - 1) * percentile) / 100;
    values[index]
}

#[tauri::command]
pub(crate) fn get_performance_metrics() -> Vec<PerformanceMetric> {
    let Ok(samples) = SAMPLES.get_or_init(|| Mutex::new(HashMap::new())).lock() else {
        return Vec::new();
    };
    let mut metrics = samples
        .iter()
        .map(|(name, values)| {
            let mut sorted = values.clone();
            sorted.sort_unstable();
            PerformanceMetric {
                name: (*name).into(),
                samples: sorted.len(),
                p50_ms: percentile(&sorted, 50),
                p95_ms: percentile(&sorted, 95),
                max_ms: sorted.last().copied().unwrap_or(0),
            }
        })
        .collect::<Vec<_>>();
    metrics.sort_by(|left, right| left.name.cmp(&right.name));
    metrics
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn percentile_is_stable_for_small_samples() {
        assert_eq!(percentile(&[10, 20, 30, 40], 50), 20);
        assert_eq!(percentile(&[10, 20, 30, 40], 95), 30);
    }
}
