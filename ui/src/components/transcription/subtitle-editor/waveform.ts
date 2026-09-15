import { WAVEFORM_PADDING } from "./constants";
import { clamp, yieldToMain } from "./utils";

export interface WaveformColumn {
  min: number;
  max: number;
}

const MAIN_THREAD_YIELD_BUDGET_MS = 8;

/** 逐采样点扫描 min/max 是纯同步计算，长音频会连续占用主线程数百毫秒，
 * 期间驱动播放头的 requestAnimationFrame 会被阻塞、错过多帧，
 * 解除阻塞后一次性读到已经前进很多的 currentTime，观感上就是播放头卡顿后突然前跳。
 * 因此按耗时切片，定期让出主线程，避免播放头同步被压住。 */
export async function buildWaveformColumns(buffer: AudioBuffer, bucketCount: number, signal?: AbortSignal) {
  const channelCount = Math.max(1, buffer.numberOfChannels);
  const channels = Array.from({ length: channelCount }, (_, index) => buffer.getChannelData(index));
  const sampleCount = channels[0]?.length || 0;
  if (sampleCount === 0) return [];

  const safeBucketCount = Math.max(1, Math.min(bucketCount, sampleCount));
  const samplesPerBucket = Math.max(1, Math.floor(sampleCount / safeBucketCount));
  const columns: WaveformColumn[] = new Array(safeBucketCount);
  let globalMax = 0;
  let sliceStartedAt = performance.now();

  for (let bucketIndex = 0; bucketIndex < safeBucketCount; bucketIndex += 1) {
    const start = bucketIndex * samplesPerBucket;
    const end = bucketIndex === safeBucketCount - 1 ? sampleCount : Math.min(sampleCount, start + samplesPerBucket);
    let min = 1;
    let max = -1;
    for (let sampleIndex = start; sampleIndex < end; sampleIndex += 1) {
      for (const channel of channels) {
        const value = channel[sampleIndex] || 0;
        if (value < min) min = value;
        if (value > max) max = value;
      }
    }
    const absPeak = Math.max(Math.abs(min), Math.abs(max));
    if (absPeak > globalMax) globalMax = absPeak;
    columns[bucketIndex] = { min, max };

    if (performance.now() - sliceStartedAt > MAIN_THREAD_YIELD_BUDGET_MS) {
      await yieldToMain();
      if (signal?.aborted) return [];
      sliceStartedAt = performance.now();
    }
  }

  if (globalMax <= 0) {
    return columns.map(() => ({ min: 0, max: 0 }));
  }

  return columns.map((column) => ({
    min: clamp(column.min / globalMax, -1, 1),
    max: clamp(column.max / globalMax, -1, 1),
  }));
}

/**
 * 把 `columns` 映射成「可视窗口内每个 CSS 像素一根竖线」的 min/max 序列。
 *
 * `columns` 覆盖的是整条时间轴 `totalWidth`，而我们只画 `[scrollLeft, scrollLeft + viewportWidth)`
 * 这一段，所以索引必须用**绝对**像素算，不能从 0 重新开始。
 */
export function sampleWaveformWindow(
  columns: WaveformColumn[],
  scrollLeft: number,
  viewportWidth: number,
  totalWidth: number,
): WaveformColumn[] {
  const width = Math.max(0, Math.round(viewportWidth));
  if (columns.length === 0 || width === 0) return [];
  const bucketsPerPx = columns.length / Math.max(1, Math.round(totalWidth));
  const offset = Math.max(0, Math.round(scrollLeft));
  const out: WaveformColumn[] = new Array(width);
  for (let x = 0; x < width; x += 1) {
    const absolute = offset + x;
    const begin = Math.floor(absolute * bucketsPerPx);
    if (begin >= columns.length) {
      out[x] = { min: 0, max: 0 };
      continue;
    }
    const end = Math.min(columns.length, Math.max(begin + 1, Math.floor((absolute + 1) * bucketsPerPx)));
    let min = 1;
    let max = -1;
    for (let i = begin; i < end; i += 1) {
      if (columns[i].min < min) min = columns[i].min;
      if (columns[i].max > max) max = columns[i].max;
    }
    out[x] = { min, max };
  }
  return out;
}

/**
 * 只绘制可视窗口的波形。
 *
 * 画布此前跟随整条时间轴的宽度（`BASE_PX_PER_SEC = 60`，缩放最高 3×），约 18 分钟
 * （100%）或 6 分钟（300%）就会触及浏览器的单边画布上限。超限后整块绘制静默变成
 * no-op，而界面只按 `columns.length > 0` 判断是否有波形，于是呈现为一片纯黑、
 * 没有任何提示。改成画布恒等于容器可视宽度、跟随 `scrollLeft` 平移重绘：既不会超限，
 * 每次重绘的描边数也固定在视口像素数，不再随音频时长线性膨胀。
 */
export function drawWaveformCanvas(
  canvas: HTMLCanvasElement | null,
  columns: WaveformColumn[],
  viewportWidth: number,
  cssHeight: number,
  waveformScale: number,
  scrollLeft: number,
  totalWidth: number,
) {
  if (!canvas) return;
  const width = Math.max(1, Math.round(viewportWidth));
  const height = Math.max(1, Math.round(cssHeight));
  if (canvas.width !== width) canvas.width = width;
  if (canvas.height !== height) canvas.height = height;
  canvas.style.width = `${width}px`;
  canvas.style.transform = `translateX(${Math.max(0, Math.round(scrollLeft))}px)`;

  const ctx = canvas.getContext("2d");
  if (!ctx) return;

  ctx.clearRect(0, 0, width, height);
  ctx.fillStyle = "rgba(10, 13, 19, 0.94)";
  ctx.fillRect(0, 0, width, height);

  ctx.strokeStyle = "rgba(255,255,255,0.08)";
  ctx.lineWidth = 1;
  ctx.beginPath();
  ctx.moveTo(0, height / 2);
  ctx.lineTo(width, height / 2);
  ctx.stroke();

  const strokes = sampleWaveformWindow(columns, scrollLeft, width, totalWidth);
  if (strokes.length === 0) return;

  ctx.strokeStyle = "rgba(139, 171, 255, 0.92)";
  ctx.lineWidth = 1;
  const amplitude = Math.max(1, (height / 2 - WAVEFORM_PADDING) * waveformScale);
  // 整个窗口合成一条路径再描边：逐像素 beginPath/stroke 是数万次独立绘制调用。
  ctx.beginPath();
  for (let x = 0; x < strokes.length; x += 1) {
    const { min, max } = strokes[x];
    ctx.moveTo(x + 0.5, height / 2 - max * amplitude);
    ctx.lineTo(x + 0.5, height / 2 - min * amplitude);
  }
  ctx.stroke();
}
