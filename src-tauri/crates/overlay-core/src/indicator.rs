/// 波形柱数量，与 IndicatorApp.tsx 的 INDICATOR_WAVE_BAR_COUNT 一致。
pub const WAVE_BAR_COUNT: usize = 9;

/// 每根波形柱的基础高度占比，与 indicator.css 的 --bar-height 逐个对应。
pub const WAVE_BAR_HEIGHTS: [f32; WAVE_BAR_COUNT] =
    [0.46, 0.58, 0.70, 0.82, 0.94, 0.82, 0.70, 0.58, 0.46];

/// 波形柱最小缩放，与 OrbWaveform 的 `max(0.18, level)` 一致。
pub const WAVE_BAR_MIN_SCALE: f32 = 0.18;

/// 感知响度曲线，移植自 ui/src/floating-orb/interaction.ts 的 floatingOrbWaveScale。
pub fn wave_scale(value: f32) -> f32 {
    // JS 侧 `Number(value) || 0` 会把 NaN 归 0。
    let normalized = if value.is_nan() {
        0.0
    } else {
        value.clamp(0.0, 1.0)
    };
    (normalized.sqrt() * 1.8).min(1.0)
}

/// 把任意数量的峰值重采样到 9 根柱，移植自 IndicatorApp.tsx 的 resampleWaveLevels。
pub fn resample_wave_levels(levels: &[f32], fallback: f32) -> [f32; WAVE_BAR_COUNT] {
    if levels.is_empty() {
        return [fallback; WAVE_BAR_COUNT];
    }
    if levels.len() == 1 {
        return [levels[0]; WAVE_BAR_COUNT];
    }
    let mut out = [0.0; WAVE_BAR_COUNT];
    for (index, slot) in out.iter_mut().enumerate() {
        let position = index as f32 * (levels.len() - 1) as f32 / (WAVE_BAR_COUNT - 1) as f32;
        let lower = position.floor() as usize;
        let upper = (levels.len() - 1).min(position.ceil() as usize);
        let progress = position - lower as f32;
        *slot = levels[lower] + (levels[upper] - levels[lower]) * progress;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wave_scale_matches_the_perceptual_loudness_curve() {
        assert_eq!(wave_scale(f32::NAN), 0.0);
        assert_eq!(wave_scale(-1.0), 0.0);
        assert_eq!(wave_scale(0.0), 0.0);
        assert_eq!(wave_scale(1.0), 1.0);
        assert_eq!(wave_scale(2.0), 1.0);
        assert!((wave_scale(0.25) - 0.9).abs() < 1e-6);
    }

    #[test]
    fn resample_fills_fallback_when_no_levels() {
        assert_eq!(resample_wave_levels(&[], 0.5), [0.5; WAVE_BAR_COUNT]);
        assert_eq!(resample_wave_levels(&[0.3], 0.0), [0.3; WAVE_BAR_COUNT]);
    }

    #[test]
    fn resample_interpolates_like_the_webview() {
        let levels = [0.0, 1.0];
        let bars = resample_wave_levels(&levels, 0.0);
        assert_eq!(bars[0], 0.0);
        assert_eq!(bars[WAVE_BAR_COUNT - 1], 1.0);
        assert!((bars[4] - 0.5).abs() < 1e-6);

        let six = [0.0, 0.2, 0.4, 0.6, 0.8, 1.0];
        let bars = resample_wave_levels(&six, 0.0);
        // 6 → 9：position = index * 5 / 8，索引 4 落在 2.5 处。
        assert!((bars[4] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn wave_bar_height_table_matches_css() {
        assert_eq!(WAVE_BAR_HEIGHTS.len(), WAVE_BAR_COUNT);
        assert_eq!(WAVE_BAR_HEIGHTS[0], 0.46);
        assert_eq!(WAVE_BAR_HEIGHTS[4], 0.94);
        assert!(WAVE_BAR_MIN_SCALE > 0.0);
    }
}
