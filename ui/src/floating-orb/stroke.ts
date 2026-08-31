const STROKE_DIAMETER_RATIO = 0.03;
const MIN_STROKE_CSS_PX = 1.25;
const MAX_STROKE_CSS_PX = 2;
const MIN_STROKE_PHYSICAL_PX = 2;

export function floatingOrbStrokeWidth(diameter: number, devicePixelRatio: number): number {
  const ratio = Number.isFinite(devicePixelRatio) && devicePixelRatio > 0 ? devicePixelRatio : 1;
  const extent = Number.isFinite(diameter) && diameter > 0 ? diameter : 0;
  const target = Math.min(MAX_STROKE_CSS_PX, Math.max(MIN_STROKE_CSS_PX, extent * STROKE_DIAMETER_RATIO));
  // CSS px 不是物理像素。低密度小球保留至少 2 个物理像素，高密度屏按比例对齐。
  return Math.max(MIN_STROKE_PHYSICAL_PX, Math.round(target * ratio)) / ratio;
}
