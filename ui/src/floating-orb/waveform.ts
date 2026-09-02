export const WAVE_BAR_COUNT = 5;

interface WaveLayoutOptions {
  barCount?: number;
  barRatio?: number;
  gapRatio?: number;
}

/** 百分比布局在不同像素相位上会把同宽细条画成不同粗细。先对齐物理像素再换回 CSS 坐标。 */
export function floatingOrbWaveLayout(
  width: number,
  left: number,
  devicePixelRatio: number,
  options: WaveLayoutOptions = {},
) {
  const ratio = Number.isFinite(devicePixelRatio) && devicePixelRatio > 0 ? devicePixelRatio : 1;
  const barCount = Math.max(1, Math.round(options.barCount ?? WAVE_BAR_COUNT));
  const bar = Math.max(1, Math.round(width * ratio * (options.barRatio ?? 0.1)));
  const gap = Math.max(1, Math.round(width * ratio * (options.gapRatio ?? 0.08)));
  const total = barCount * bar + (barCount - 1) * gap;
  const start = Math.round(left * ratio + (width * ratio - total) / 2);
  return {
    width: bar / ratio,
    offsets: Array.from({ length: barCount }, (_, index) =>
      (start + index * (bar + gap)) / ratio - left),
  };
}
