export const WAVE_BAR_COUNT = 5;

/** 百分比布局在不同像素相位上会把同宽细条画成不同粗细。先对齐物理像素再换回 CSS 坐标。 */
export function floatingOrbWaveLayout(width: number, left: number, devicePixelRatio: number) {
  const ratio = Number.isFinite(devicePixelRatio) && devicePixelRatio > 0 ? devicePixelRatio : 1;
  const bar = Math.max(1, Math.round(width * ratio * 0.1));
  const gap = Math.max(1, Math.round(width * ratio * 0.08));
  const total = WAVE_BAR_COUNT * bar + (WAVE_BAR_COUNT - 1) * gap;
  const start = Math.round(left * ratio + (width * ratio - total) / 2);
  return {
    width: bar / ratio,
    offsets: Array.from({ length: WAVE_BAR_COUNT }, (_, index) =>
      (start + index * (bar + gap)) / ratio - left),
  };
}
