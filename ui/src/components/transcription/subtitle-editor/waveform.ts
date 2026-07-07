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

export function drawWaveformCanvas(
  canvas: HTMLCanvasElement | null,
  columns: WaveformColumn[],
  cssWidth: number,
  cssHeight: number,
  waveformScale: number,
) {
  if (!canvas) return;
  const width = Math.max(1, Math.round(cssWidth));
  const height = Math.max(1, Math.round(cssHeight));
  if (canvas.width !== width) canvas.width = width;
  if (canvas.height !== height) canvas.height = height;

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

  if (columns.length === 0) return;

  ctx.strokeStyle = "rgba(139, 171, 255, 0.92)";
  ctx.lineWidth = 1;
  const amplitude = Math.max(1, (height / 2 - WAVEFORM_PADDING) * waveformScale);
  const bucketSize = columns.length / width;
  for (let x = 0; x < width; x += 1) {
    const begin = Math.floor(x * bucketSize);
    const end = Math.max(begin + 1, Math.floor((x + 1) * bucketSize));
    let min = 1;
    let max = -1;
    for (let i = begin; i < end && i < columns.length; i += 1) {
      min = Math.min(min, columns[i].min);
      max = Math.max(max, columns[i].max);
    }
    const y1 = height / 2 - max * amplitude;
    const y2 = height / 2 - min * amplitude;
    ctx.beginPath();
    ctx.moveTo(x + 0.5, y1);
    ctx.lineTo(x + 0.5, y2);
    ctx.stroke();
  }
}
