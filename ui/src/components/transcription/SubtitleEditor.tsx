import { useEffect, useMemo, useRef, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { Button } from "@/components/ui/Button";
import { cn } from "@/lib/cn";
import {
  formatClock,
  parseClock,
  type EditableCue,
} from "@/features/transcription/subtitles";

const BASE_PX_PER_SEC = 60;
const MIN_CUE_MS = 100;
const NUDGE_MS = 100;
/** 默认间隙合并阈值：参考 Netflix 字幕规范「相邻字幕至少间隔 2 帧」（24~30fps 约 66~83ms），
 * 小于该间隔人眼会感知为闪烁而非有意的切换停顿；留出余量取 200ms 作为默认阈值。 */
const DEFAULT_GAP_MERGE_MS = 200;
const SNAP_DISTANCE_PX = 8;
const RATE_OPTIONS = [0.75, 1, 1.25, 1.5];
const TIMELINE_ZOOM_LEVELS = [0.5, 0.75, 1, 1.5, 2, 3];
const WAVEFORM_ZOOM_LEVELS = [0.5, 0.75, 1, 1.5, 2, 3];
const TIMELINE_HEIGHT = 118;
const WAVEFORM_HEIGHT = 52;
const WAVEFORM_TOP = 24;
const WAVEFORM_PADDING = 7;
const CUE_LANE_TOP = 82;
const CUE_LANE_HEIGHT = 26;
const CUE_BLOCK_TOP = 81;
const MIN_WAVEFORM_BUCKETS = 240;
const MAX_WAVEFORM_BUCKETS = 6000;
/** 播放头与媒体时钟漂移超过该值视为主动 seek 等真实跳变，直接硬对齐。 */
const PLAYHEAD_HARD_SNAP_MS = 300;
/** 每帧允许用于追平漂移的速度占比：0.15 表示播放头最快以 0.85x/1.15x 的速度缓慢校准，视觉不可察觉。 */
const PLAYHEAD_MAX_CORRECTION_RATIO = 0.15;

type DragMode = "move" | "left" | "right";

interface DragState {
  id: string;
  mode: DragMode;
  startX: number;
  beginMs: number;
  endMs: number;
}

interface PanState {
  startX: number;
  startScrollLeft: number;
}

interface WaveformColumn {
  min: number;
  max: number;
}

interface CueNeighbors {
  prevEnd: number;
  nextBegin: number;
}

function clamp(value: number, min: number, max: number) {
  return Math.min(max, Math.max(min, value));
}

function formatZoom(scale: number) {
  return `${Math.round(scale * 100)}%`;
}

function isTypingTarget(target: EventTarget | null) {
  if (!(target instanceof HTMLElement)) return false;
  if (target.isContentEditable) return true;
  const tagName = target.tagName;
  // 不含 BUTTON：编辑页内空格键始终用于播放/暂停，不应被"此前点过的按钮仍持有焦点"劫持
  // （典型场景：点击窗口标题栏的最大化按钮后再按空格，浏览器会把空格当成对该按钮的默认点击）。
  return tagName === "INPUT" || tagName === "TEXTAREA" || tagName === "SELECT";
}

function isInteractiveTarget(target: EventTarget | null) {
  return target instanceof HTMLElement
    && !!target.closest("button, input, textarea, select, a, label");
}

const MAIN_THREAD_YIELD_BUDGET_MS = 8;

function yieldToMain() {
  return new Promise<void>((resolve) => setTimeout(resolve, 0));
}

/** 逐采样点扫描 min/max 是纯同步计算，长音频会连续占用主线程数百毫秒，
 * 期间驱动播放头的 requestAnimationFrame 会被阻塞、错过多帧，
 * 解除阻塞后一次性读到已经前进很多的 currentTime，观感上就是播放头卡顿后突然前跳。
 * 因此按耗时切片，定期让出主线程，避免播放头同步被压住。 */
async function buildWaveformColumns(buffer: AudioBuffer, bucketCount: number, signal?: AbortSignal) {
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

function drawWaveformCanvas(
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

function joinTexts(a: string, b: string) {
  const left = a.trimEnd();
  const right = b.trimStart();
  if (!left) return right;
  if (!right) return left;
  return /[a-zA-Z0-9]$/.test(left) && /^[a-zA-Z0-9]/.test(right) ? `${left} ${right}` : `${left}${right}`;
}

function TimeInput({
  valueMs,
  onCommit,
  title,
}: {
  valueMs: number;
  onCommit: (ms: number) => void;
  title: string;
}) {
  const [draft, setDraft] = useState<string | null>(null);
  return (
    <input
      type="text"
      title={title}
      value={draft ?? formatClock(valueMs)}
      onChange={(event) => setDraft(event.target.value)}
      onFocus={(event) => event.target.select()}
      onBlur={() => {
        if (draft !== null) {
          const ms = parseClock(draft);
          if (ms !== null) onCommit(ms);
          setDraft(null);
        }
      }}
      onKeyDown={(event) => {
        if (event.key === "Enter") event.currentTarget.blur();
        if (event.key === "Escape") {
          setDraft(null);
          event.currentTarget.blur();
        }
      }}
      className={cn(
        "h-7 w-[5.75rem] rounded-[var(--radius-sm)] border border-[var(--color-line)] bg-[var(--color-surface)]",
        "text-center font-mono text-xs tabular-nums text-[var(--color-fg-muted)]",
        "transition-colors duration-[var(--dur-fast)] focus:outline-none focus:border-[var(--accent-ring)]",
      )}
    />
  );
}

function CueTextarea({
  value,
  onChange,
  textareaRef,
}: {
  value: string;
  onChange: (text: string) => void;
  textareaRef?: (node: HTMLTextAreaElement | null) => void;
}) {
  const localRef = useRef<HTMLTextAreaElement | null>(null);
  useEffect(() => {
    const el = localRef.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${el.scrollHeight}px`;
  }, [value]);

  return (
    <textarea
      ref={(node) => {
        localRef.current = node;
        textareaRef?.(node);
      }}
      value={value}
      rows={1}
      onChange={(event) => onChange(event.target.value)}
      placeholder="（空字幕）"
      className={cn(
        "mt-1.5 w-full resize-none overflow-hidden rounded-[var(--radius-sm)] border border-transparent bg-transparent px-2 py-1 text-sm leading-6",
        "text-[var(--color-fg-muted)] transition-colors duration-[var(--dur-fast)] placeholder:text-[var(--color-fg-faint)]",
        "hover:border-[var(--color-line)] focus:border-[var(--accent-ring)] focus:bg-[var(--color-surface)] focus:outline-none",
      )}
    />
  );
}

function TimeControl({
  label,
  valueMs,
  onCommit,
  onSetPlayhead,
}: {
  label: string;
  valueMs: number;
  onCommit: (ms: number) => void;
  onSetPlayhead: () => void;
}) {
  const iconButton =
    "flex h-7 w-6 items-center justify-center rounded-[var(--radius-sm)] border border-transparent text-xs " +
    "text-[var(--color-fg-subtle)] transition-colors duration-[var(--dur-fast)] hover:border-[var(--color-line)] " +
    "hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-fg)] focus:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent-ring)]";
  return (
    <span className="inline-flex items-center gap-0.5">
      <TimeInput valueMs={valueMs} onCommit={onCommit} title={`${label}时间（mm:ss.mmm，回车确认）`} />
      <button type="button" title={`${label}设为播放头位置`} className={iconButton} onClick={onSetPlayhead}>
        <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth={1.5} strokeLinecap="round" className="h-3.5 w-3.5" aria-hidden>
          <path d="M8 2v12" />
          <circle cx="8" cy="8" r="3.2" />
        </svg>
      </button>
      <button type="button" title={`${label} -0.1 秒`} className={iconButton} onClick={() => onCommit(Math.max(0, valueMs - NUDGE_MS))}>
        −
      </button>
      <button type="button" title={`${label} +0.1 秒`} className={iconButton} onClick={() => onCommit(valueMs + NUDGE_MS)}>
        +
      </button>
    </span>
  );
}

export function SubtitleEditor({
  mediaPath,
  cues,
  onCuesChange,
  footer,
}: {
  mediaPath: string | null;
  cues: EditableCue[];
  onCuesChange: (next: EditableCue[]) => void;
  footer?: React.ReactNode;
}) {
  const audioRef = useRef<HTMLAudioElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const timelineRef = useRef<HTMLDivElement>(null);
  const waveformCanvasRef = useRef<HTMLCanvasElement>(null);
  const dragRef = useRef<DragState | null>(null);
  const panRef = useRef<PanState | null>(null);
  const renderFrameRef = useRef(0);
  const playheadClockRef = useRef<{ displayMs: number; frameAt: number } | null>(null);
  const zoomAnchorRef = useRef<{ timeMs: number; offsetX: number } | null>(null);
  const cueTextRefs = useRef<Record<string, HTMLTextAreaElement | null>>({});

  const [playing, setPlaying] = useState(false);
  const [currentMs, setCurrentMs] = useState(0);
  const [durationMs, setDurationMs] = useState(0);
  const [rate, setRate] = useState(1);
  const [timelineZoom, setTimelineZoom] = useState(1);
  const [waveformZoom, setWaveformZoom] = useState(1);
  const [selectedCueId, setSelectedCueId] = useState<string | null>(null);
  const [playbackError, setPlaybackError] = useState(false);
  const [waveformColumns, setWaveformColumns] = useState<WaveformColumn[]>([]);
  const [waveformLoading, setWaveformLoading] = useState(false);
  const [gapMergeMs, setGapMergeMs] = useState(DEFAULT_GAP_MERGE_MS);

  const mediaSrc = useMemo(() => (mediaPath ? convertFileSrc(mediaPath) : ""), [mediaPath]);
  const lastEndMs = useMemo(() => cues.reduce((max, cue) => Math.max(max, cue.endMs), 0), [cues]);
  const totalMs = Math.max(durationMs, lastEndMs);
  const pxPerSec = BASE_PX_PER_SEC * timelineZoom;
  const timelineWidth = Math.max(320, Math.ceil((totalMs / 1000) * pxPerSec) + 80);
  const playheadLeft = (currentMs / 1000) * pxPerSec;
  const activeCueId = useMemo(() => {
    const active = cues.find((cue) => currentMs >= cue.beginMs && currentMs < cue.endMs);
    return active?.id ?? null;
  }, [cues, currentMs]);
  const highlightedCueId = activeCueId ?? selectedCueId;
  const timelineZoomIndex = TIMELINE_ZOOM_LEVELS.indexOf(timelineZoom);
  const waveformZoomIndex = WAVEFORM_ZOOM_LEVELS.indexOf(waveformZoom);
  const secondLabels = useMemo(() => {
    const labels: number[] = [];
    for (let s = 0; s <= Math.ceil(totalMs / 1000); s += 10) labels.push(s);
    return labels;
  }, [totalMs]);

  const stopPlayheadSync = () => {
    playheadClockRef.current = null;
    if (!renderFrameRef.current) return;
    cancelAnimationFrame(renderFrameRef.current);
    renderFrameRef.current = 0;
  };

  /** 播放头不直接镜像 audio.currentTime：Chromium 暂停后再 play() 需要重启音频输出管线，
   * 期间媒体时钟先停滞（约 50~200ms），管线就绪后又带补偿地跳步前进；从 0 首播因管线已预热而无此现象。
   * 直接镜像的观感就是"暂停后再播放，播放头先顿一下再突然跳到前面"，且无法靠改读取时机修复。
   * 因此播放期间用 rAF 壁钟推进播放头，媒体时钟只作为漂移校准源：
   * 小漂移每帧限幅缓慢追平（同时吸收 currentTime 偶发的几毫秒微小回退），
   * 只有超过 PLAYHEAD_HARD_SNAP_MS 的真实跳变（主动 seek 等）才硬对齐。 */
  const syncPlayhead = (frameAt: number) => {
    const audio = audioRef.current;
    if (!audio) return;
    const audioMs = audio.currentTime * 1000;
    const clock = playheadClockRef.current ?? { displayMs: audioMs, frameAt };
    const elapsedMs = Math.max(0, frameAt - clock.frameAt);
    let displayMs = clock.displayMs + elapsedMs * audio.playbackRate;
    const driftMs = audioMs - displayMs;
    if (Math.abs(driftMs) > PLAYHEAD_HARD_SNAP_MS) {
      displayMs = audioMs;
    } else {
      const maxCorrection = elapsedMs * audio.playbackRate * PLAYHEAD_MAX_CORRECTION_RATIO;
      displayMs += clamp(driftMs, -maxCorrection, maxCorrection);
    }
    playheadClockRef.current = { displayMs, frameAt };
    setCurrentMs(displayMs);
    if (!audio.paused && !audio.ended) {
      renderFrameRef.current = requestAnimationFrame(syncPlayhead);
    } else {
      renderFrameRef.current = 0;
    }
  };

  const scrollCueIntoView = (cue: EditableCue, behavior: ScrollBehavior = "smooth") => {
    const container = timelineRef.current;
    if (!container) return;
    const left = (cue.beginMs / 1000) * pxPerSec;
    const right = (cue.endMs / 1000) * pxPerSec;
    if (left >= container.scrollLeft && right <= container.scrollLeft + container.clientWidth) return;
    const targetLeft = Math.max(0, left - container.clientWidth * 0.2);
    container.scrollTo({ left: targetLeft, behavior });
  };

  const seek = (ms: number) => {
    const target = Math.max(0, totalMs > 0 ? Math.min(ms, totalMs) : ms);
    // 主动 seek 后壁钟锚点已失效，置空让下一帧从新的 currentTime 重新起步
    playheadClockRef.current = null;
    setCurrentMs(target);
    const audio = audioRef.current;
    if (audio && !playbackError && Number.isFinite(audio.duration)) {
      audio.currentTime = target / 1000;
    }
  };

  const focusCueEditor = (cueId: string) => {
    setSelectedCueId(cueId);
    const row = listRef.current?.querySelector<HTMLElement>(`[data-cue-id="${cueId}"]`);
    row?.scrollIntoView({ block: "nearest", behavior: "smooth" });
    requestAnimationFrame(() => {
      const input = cueTextRefs.current[cueId];
      if (!input) return;
      input.focus();
      input.select();
    });
  };

  const selectCue = (cue: EditableCue, behavior: ScrollBehavior = "smooth") => {
    setSelectedCueId(cue.id);
    seek(cue.beginMs);
    scrollCueIntoView(cue, behavior);
  };

  const snapThresholdMs = Math.max(40, (SNAP_DISTANCE_PX / pxPerSec) * 1000);
  const snapValue = (value: number, candidates: number[]) => {
    let snapped = value;
    let distance = snapThresholdMs + 1;
    for (const candidate of candidates) {
      const diff = Math.abs(candidate - value);
      if (diff <= snapThresholdMs && diff < distance) {
        snapped = candidate;
        distance = diff;
      }
    }
    return snapped;
  };

  const cueIndexOf = (cueId: string) => cues.findIndex((cue) => cue.id === cueId);

  const cueNeighborsOf = (index: number): CueNeighbors => ({
    prevEnd: index > 0 ? cues[index - 1].endMs : 0,
    nextBegin: index < cues.length - 1 ? cues[index + 1].beginMs : Number.POSITIVE_INFINITY,
  });

  const setCueWindow = (index: number, beginMs: number, endMs: number) => {
    onCuesChange(
      cues.map((cue, cueIndex) => (cueIndex === index ? { ...cue, beginMs, endMs } : cue)),
    );
  };

  const moveCueWindow = (index: number, nextBeginMs: number) => {
    const cue = cues[index];
    if (!cue) return;
    const { prevEnd, nextBegin } = cueNeighborsOf(index);
    const duration = cue.endMs - cue.beginMs;
    const maxBegin = Number.isFinite(nextBegin) ? nextBegin - duration : Number.POSITIVE_INFINITY;
    const clamped = clamp(
      nextBeginMs,
      prevEnd,
      Number.isFinite(maxBegin) ? maxBegin : nextBeginMs,
    );
    const snapped = snapValue(
      clamped,
      [prevEnd, Number.isFinite(nextBegin) ? nextBegin - duration : clamped].filter(Number.isFinite),
    );
    setCueWindow(index, snapped, snapped + duration);
  };

  const resizeCueLeft = (index: number, nextBeginMs: number) => {
    const cue = cues[index];
    if (!cue) return;
    const { prevEnd } = cueNeighborsOf(index);
    const clamped = clamp(nextBeginMs, prevEnd, cue.endMs - MIN_CUE_MS);
    const snapped = snapValue(clamped, [prevEnd]);
    setCueWindow(index, snapped, cue.endMs);
  };

  const resizeCueRight = (index: number, nextEndMs: number) => {
    const cue = cues[index];
    if (!cue) return;
    const { nextBegin } = cueNeighborsOf(index);
    const clamped = clamp(
      nextEndMs,
      cue.beginMs + MIN_CUE_MS,
      Number.isFinite(nextBegin) ? nextBegin : nextEndMs,
    );
    const snapped = snapValue(
      clamped,
      [Number.isFinite(nextBegin) ? nextBegin : clamped].filter(Number.isFinite),
    );
    setCueWindow(index, cue.beginMs, snapped);
  };

  const updateCueText = (cueId: string, patch: Partial<EditableCue>) => {
    onCuesChange(cues.map((cue) => (cue.id === cueId ? { ...cue, ...patch } : cue)));
  };

  const togglePlay = async () => {
    const audio = audioRef.current;
    if (!audio || playbackError) return;
    if (playing) {
      audio.pause();
      return;
    }
    try {
      audio.playbackRate = rate;
      await audio.play();
    } catch {
      setPlaybackError(true);
    }
  };

  const changeTimelineZoom = (step: -1 | 1, anchorClientX?: number) => {
    const nextIndex = clamp(timelineZoomIndex + step, 0, TIMELINE_ZOOM_LEVELS.length - 1);
    const nextZoom = TIMELINE_ZOOM_LEVELS[nextIndex];
    if (nextZoom === timelineZoom) return;
    const container = timelineRef.current;
    if (container && anchorClientX !== undefined) {
      const rect = container.getBoundingClientRect();
      const offsetX = anchorClientX - rect.left;
      const timeMs = ((container.scrollLeft + offsetX) / pxPerSec) * 1000;
      zoomAnchorRef.current = { timeMs, offsetX };
    }
    setTimelineZoom(nextZoom);
  };

  const changeWaveformZoom = (step: -1 | 1) => {
    const nextIndex = clamp(waveformZoomIndex + step, 0, WAVEFORM_ZOOM_LEVELS.length - 1);
    setWaveformZoom(WAVEFORM_ZOOM_LEVELS[nextIndex]);
  };

  const cycleRate = () => {
    const next = RATE_OPTIONS[(RATE_OPTIONS.indexOf(rate) + 1) % RATE_OPTIONS.length];
    setRate(next);
    if (audioRef.current) audioRef.current.playbackRate = next;
  };

  const deleteCue = (cueId: string) => {
    onCuesChange(cues.filter((cue) => cue.id !== cueId));
    if (selectedCueId === cueId) setSelectedCueId(null);
  };

  const splitCue = (cue: EditableCue) => {
    const index = cueIndexOf(cue.id);
    if (index < 0) return;
    const inRange = currentMs > cue.beginMs + MIN_CUE_MS && currentMs < cue.endMs - MIN_CUE_MS;
    const splitMs = inRange ? currentMs : Math.round((cue.beginMs + cue.endMs) / 2);
    const ratio = (splitMs - cue.beginMs) / (cue.endMs - cue.beginMs);
    const chars = Array.from(cue.text);
    const cut = clamp(Math.round(chars.length * ratio), 0, chars.length);
    const first: EditableCue = { ...cue, endMs: splitMs, text: chars.slice(0, cut).join("").trim() };
    const second: EditableCue = {
      ...cue,
      id: `${cue.id}-s${Date.now().toString(36)}`,
      beginMs: splitMs,
      text: chars.slice(cut).join("").trim(),
      badge: undefined,
    };
    onCuesChange([
      ...cues.slice(0, index),
      first,
      second,
      ...cues.slice(index + 1),
    ]);
    setSelectedCueId(second.id);
  };

  const mergeWithNext = (cue: EditableCue) => {
    const index = cueIndexOf(cue.id);
    const next = cues[index + 1];
    if (index < 0 || !next) return;
    const merged: EditableCue = {
      ...cue,
      endMs: Math.max(cue.endMs, next.endMs),
      text: joinTexts(cue.text, next.text),
    };
    onCuesChange([...cues.slice(0, index), merged, ...cues.slice(index + 2)]);
    setSelectedCueId(merged.id);
  };

  const mergeGaps = () => {
    const threshold = Math.max(0, gapMergeMs);
    let changed = false;
    const next = cues.map((cue, index) => {
      const nextCue = cues[index + 1];
      if (nextCue) {
        const gap = nextCue.beginMs - cue.endMs;
        if (gap > 0 && gap < threshold) {
          changed = true;
          return { ...cue, endMs: nextCue.beginMs };
        }
      }
      return cue;
    });
    if (changed) onCuesChange(next);
  };

  const insertAfter = (cue: EditableCue) => {
    const index = cueIndexOf(cue.id);
    if (index < 0) return;
    const next = cues[index + 1];
    const beginMs = cue.endMs;
    const endMs = next
      ? Math.max(beginMs + MIN_CUE_MS, Math.min(next.beginMs, beginMs + 2000))
      : beginMs + 2000;
    const inserted: EditableCue = {
      id: `${cue.id}-i${Date.now().toString(36)}`,
      beginMs,
      endMs,
      text: "",
    };
    onCuesChange([...cues.slice(0, index + 1), inserted, ...cues.slice(index + 1)]);
    setSelectedCueId(inserted.id);
    requestAnimationFrame(() => focusCueEditor(inserted.id));
  };

  const onCuePointerDown = (
    event: React.PointerEvent,
    cue: EditableCue,
    mode: DragMode,
  ) => {
    if (event.button !== 0) return;
    event.preventDefault();
    event.stopPropagation();
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    dragRef.current = {
      id: cue.id,
      mode,
      startX: event.clientX,
      beginMs: cue.beginMs,
      endMs: cue.endMs,
    };
    setSelectedCueId(cue.id);
  };

  const onTimelinePointerDown = (event: React.PointerEvent<HTMLDivElement>) => {
    const container = timelineRef.current;
    if (!container) return;
    if (event.button === 1) {
      event.preventDefault();
      event.stopPropagation();
      (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
      panRef.current = { startX: event.clientX, startScrollLeft: container.scrollLeft };
      return;
    }
    if (event.button !== 0) return;
    const rect = container.getBoundingClientRect();
    const x = event.clientX - rect.left + container.scrollLeft;
    seek((x / pxPerSec) * 1000);
  };

  const onTimelinePointerMove = (event: React.PointerEvent<HTMLDivElement>) => {
    const pan = panRef.current;
    if (pan && timelineRef.current) {
      const nextScrollLeft = pan.startScrollLeft - (event.clientX - pan.startX);
      timelineRef.current.scrollLeft = Math.max(0, nextScrollLeft);
      return;
    }

    const drag = dragRef.current;
    if (!drag) return;
    const index = cueIndexOf(drag.id);
    if (index < 0) return;
    const deltaMs = ((event.clientX - drag.startX) / pxPerSec) * 1000;
    if (drag.mode === "move") {
      moveCueWindow(index, drag.beginMs + deltaMs);
    } else if (drag.mode === "left") {
      resizeCueLeft(index, drag.beginMs + deltaMs);
    } else {
      resizeCueRight(index, drag.endMs + deltaMs);
    }
  };

  const onTimelinePointerUp = () => {
    dragRef.current = null;
    panRef.current = null;
  };

  const onTimelineWheel = (event: React.WheelEvent<HTMLDivElement>) => {
    if (event.ctrlKey) {
      event.preventDefault();
      changeTimelineZoom(event.deltaY < 0 ? 1 : -1, event.clientX);
      return;
    }
    if (event.altKey) {
      event.preventDefault();
      changeWaveformZoom(event.deltaY < 0 ? 1 : -1);
    }
  };

  useEffect(() => {
    stopPlayheadSync();
    setPlaying(false);
    setCurrentMs(0);
    setDurationMs(0);
    setPlaybackError(false);
    setWaveformColumns([]);
    setWaveformLoading(false);
    setSelectedCueId(null);
  }, [mediaSrc]);

  useEffect(() => {
    if (!mediaSrc) return;
    let disposed = false;
    const controller = new AbortController();
    const loadWaveform = async () => {
      setWaveformLoading(true);
      try {
        const response = await fetch(mediaSrc, { signal: controller.signal });
        const bytes = await response.arrayBuffer();
        if (disposed) return;
        const audioContext = new AudioContext();
        try {
          const buffer = await audioContext.decodeAudioData(bytes.slice(0));
          if (disposed) return;
          const bucketCount = Math.max(
            MIN_WAVEFORM_BUCKETS,
            Math.min(
              MAX_WAVEFORM_BUCKETS,
              Math.round(
                buffer.duration
                * BASE_PX_PER_SEC
                * TIMELINE_ZOOM_LEVELS[TIMELINE_ZOOM_LEVELS.length - 1]
                * 1.35,
              ),
            ),
          );
          const nextColumns = await buildWaveformColumns(buffer, bucketCount, controller.signal);
          if (disposed) return;
          setWaveformColumns(nextColumns);
          setDurationMs((current) => current || Math.round(buffer.duration * 1000));
        } finally {
          void audioContext.close().catch(() => {});
        }
      } catch {
        if (!disposed) setWaveformColumns([]);
      } finally {
        if (!disposed) setWaveformLoading(false);
      }
    };
    void loadWaveform();
    return () => {
      disposed = true;
      controller.abort();
    };
  }, [mediaSrc]);

  useEffect(() => () => stopPlayheadSync(), []);

  useEffect(() => {
    if (audioRef.current) audioRef.current.playbackRate = rate;
  }, [rate]);

  useEffect(() => {
    drawWaveformCanvas(
      waveformCanvasRef.current,
      waveformColumns,
      timelineWidth,
      WAVEFORM_HEIGHT,
      waveformZoom,
    );
  }, [timelineWidth, waveformColumns, waveformZoom]);

  useEffect(() => {
    const anchor = zoomAnchorRef.current;
    if (!anchor || !timelineRef.current) return;
    timelineRef.current.scrollLeft = Math.max(
      0,
      (anchor.timeMs / 1000) * pxPerSec - anchor.offsetX,
    );
    zoomAnchorRef.current = null;
  }, [pxPerSec]);

  useEffect(() => {
    const onWindowKeydown = (event: KeyboardEvent) => {
      if (event.code !== "Space" || event.repeat || event.ctrlKey || event.altKey || event.metaKey) return;
      if (isTypingTarget(event.target)) return;
      if (!mediaSrc || playbackError) return;
      event.preventDefault();
      void togglePlay();
    };
    window.addEventListener("keydown", onWindowKeydown, true);
    return () => window.removeEventListener("keydown", onWindowKeydown, true);
  }, [mediaSrc, playbackError, playing, rate]);

  useEffect(() => {
    if (!playing || !activeCueId) return;
    listRef.current
      ?.querySelector(`[data-cue-id="${activeCueId}"]`)
      ?.scrollIntoView({ block: "nearest", behavior: "smooth" });
  }, [playing, activeCueId]);

  useEffect(() => {
    if (!playing || !timelineRef.current) return;
    const container = timelineRef.current;
    if (playheadLeft < container.scrollLeft + 48 || playheadLeft > container.scrollLeft + container.clientWidth - 88) {
      // 播放期间该 effect 会随播放头每帧变化重复触发；用 "smooth" 会在这个高频调用下
      // 反复打断、重启浏览器原生的滚动缓动动画，表现为播放头突然"跳"一下。
      // 改为瞬时跳转后，连续高频的小步纠正在视觉上本身就是连贯的，不需要额外动画。
      container.scrollLeft = Math.max(0, playheadLeft - container.clientWidth / 3);
    }
  }, [playing, playheadLeft]);

  const rowIconButton =
    "flex h-7 w-7 items-center justify-center rounded-[var(--radius-sm)] border border-transparent text-[var(--color-fg-subtle)] " +
    "transition-colors duration-[var(--dur-fast)] hover:border-[var(--color-line)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-fg)] " +
    "focus:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent-ring)] disabled:cursor-not-allowed disabled:opacity-35";

  return (
    <div className="flex flex-col overflow-hidden rounded-[var(--radius-lg)] border border-[var(--color-line)] bg-[var(--color-surface)]">
      {mediaSrc && (
        <audio
          ref={audioRef}
          src={mediaSrc}
          preload="metadata"
          onLoadedMetadata={(event) => {
            const duration = event.currentTarget.duration;
            if (Number.isFinite(duration)) setDurationMs(Math.round(duration * 1000));
          }}
          onTimeUpdate={(event) => {
            if (!playing) setCurrentMs(event.currentTarget.currentTime * 1000);
          }}
          onPlay={() => {
            setPlaying(true);
            stopPlayheadSync();
            renderFrameRef.current = requestAnimationFrame(syncPlayhead);
          }}
          onPause={(event) => {
            stopPlayheadSync();
            setPlaying(false);
            setCurrentMs(event.currentTarget.currentTime * 1000);
          }}
          onEnded={(event) => {
            stopPlayheadSync();
            setPlaying(false);
            setCurrentMs(event.currentTarget.currentTime * 1000);
          }}
          onError={() => setPlaybackError(true)}
        />
      )}

      <div className="flex flex-wrap items-center gap-3 border-b border-[var(--color-line)] px-4 py-3">
        <Button size="sm" variant="primary" onClick={togglePlay} disabled={!mediaSrc || playbackError} className="w-16">
          {playing ? "暂停" : "播放"}
        </Button>
        <Button size="sm" onClick={() => seek(currentMs - 5000)} disabled={!mediaSrc || playbackError} title="后退 5 秒">
          −5s
        </Button>
        <Button size="sm" onClick={() => seek(currentMs + 5000)} disabled={!mediaSrc || playbackError} title="前进 5 秒">
          +5s
        </Button>
        <Button size="sm" onClick={cycleRate} disabled={!mediaSrc || playbackError} title="播放速度" className="w-14 font-mono tabular-nums">
          {rate}×
        </Button>
        <span className="inline-flex items-center gap-1 rounded-[var(--radius-pill)] border border-[var(--color-line)] px-2 py-1 font-mono text-[11px] text-[var(--color-fg-faint)]">
          时间轴 {formatZoom(timelineZoom)}
          <span className="text-[var(--color-fg-subtle)]">Ctrl+滚轮</span>
        </span>
        <span className="inline-flex items-center gap-1 rounded-[var(--radius-pill)] border border-[var(--color-line)] px-2 py-1 font-mono text-[11px] text-[var(--color-fg-faint)]">
          波形 {formatZoom(waveformZoom)}
          <span className="text-[var(--color-fg-subtle)]">Alt+滚轮</span>
        </span>
        <span
          className="inline-flex items-center gap-1.5 rounded-[var(--radius-pill)] border border-[var(--color-line)] px-2 py-1"
          title="小于该阈值的相邻字幕间隙会被合并（前一条延伸至后一条开始），避免烧录成片后字幕出现闪烁"
        >
          <span className="text-[11px] text-[var(--color-fg-faint)]">间隙 ≤</span>
          <input
            type="number"
            min={0}
            step={10}
            value={gapMergeMs}
            onChange={(event) => setGapMergeMs(Math.max(0, Number(event.target.value) || 0))}
            className="h-6 w-14 rounded-[var(--radius-sm)] border border-[var(--color-line)] bg-[var(--color-surface)] text-center font-mono text-[11px] tabular-nums text-[var(--color-fg-muted)] focus:outline-none focus:border-[var(--accent-ring)]"
          />
          <span className="text-[11px] text-[var(--color-fg-faint)]">ms</span>
        </span>
        <Button size="sm" onClick={mergeGaps} disabled={cues.length < 2}>
          合并字幕间隙
        </Button>
        <span className="rounded-[var(--radius-pill)] border border-[var(--color-line)] px-2 py-1 font-mono text-[11px] leading-none text-[var(--color-fg-faint)]">
          Space 播放/暂停
        </span>
        <span className="rounded-[var(--radius-pill)] border border-[var(--color-line)] px-2 py-1 font-mono text-[11px] leading-none text-[var(--color-fg-faint)]">
          中键拖动时间轴
        </span>
        <span className="font-mono text-xs tabular-nums text-[var(--color-fg-subtle)]">
          {formatClock(currentMs)} / {formatClock(totalMs)}
        </span>
        <input
          type="range"
          min={0}
          max={Math.max(1, totalMs)}
          step={10}
          value={clamp(currentMs, 0, Math.max(1, totalMs))}
          onChange={(event) => seek(Number(event.target.value))}
          disabled={totalMs === 0}
          className="min-w-40 flex-1"
          aria-label="播放进度"
        />
      </div>

      {playbackError && (
        <p className="border-b border-[var(--color-line)] bg-[color-mix(in_srgb,var(--color-warn)_8%,transparent)] px-4 py-2 text-xs text-[var(--color-warn)]">
          此文件格式无法在应用内预览播放（如 mkv/wmv/amr 等容器），字幕编辑不受影响。
        </p>
      )}

      <div
        ref={timelineRef}
        className="relative overflow-x-auto border-b border-[var(--color-line)] bg-[var(--color-bg)]"
        onPointerDown={onTimelinePointerDown}
        onPointerMove={onTimelinePointerMove}
        onPointerUp={onTimelinePointerUp}
        onPointerCancel={onTimelinePointerUp}
        onWheel={onTimelineWheel}
      >
        <div className="relative select-none" style={{ width: `${timelineWidth}px`, height: `${TIMELINE_HEIGHT}px` }}>
          <div
            className="absolute inset-x-0 top-0 h-5 border-b border-[var(--color-line)]"
            style={{
              backgroundImage:
                "repeating-linear-gradient(to right, var(--color-line) 0 1px, transparent 1px " + `${pxPerSec}px)`,
            }}
          >
            {secondLabels.map((s) => (
              <span
                key={s}
                className="absolute top-0.5 font-mono text-[10px] tabular-nums text-[var(--color-fg-faint)]"
                style={{ left: `${s * pxPerSec + 3}px` }}
              >
                {formatClock(s * 1000).replace(/\.\d+$/, "")}
              </span>
            ))}
          </div>

          <div className="absolute inset-x-0 rounded-[var(--radius-md)] border border-[var(--color-line)] bg-[linear-gradient(180deg,rgba(17,23,34,0.9),rgba(10,13,19,0.96))]" style={{ top: `${WAVEFORM_TOP}px`, height: `${WAVEFORM_HEIGHT}px` }}>
            {waveformColumns.length > 0 ? (
              <canvas
                ref={waveformCanvasRef}
                className="block h-full w-full"
                style={{ width: `${timelineWidth}px`, height: `${WAVEFORM_HEIGHT}px` }}
              />
            ) : (
              <div className="flex h-full items-center justify-center text-[10px] text-[var(--color-fg-faint)]">
                {waveformLoading ? "正在生成波形…" : "当前文件暂不支持波形预览"}
              </div>
            )}
          </div>

          <div className="absolute inset-x-0 rounded-[var(--radius-md)] border border-[color-mix(in_srgb,var(--color-line)_78%,transparent)] bg-[rgba(10,13,19,0.75)]" style={{ top: `${CUE_LANE_TOP}px`, height: `${CUE_LANE_HEIGHT}px` }} />

          {cues.map((cue) => {
            const left = (cue.beginMs / 1000) * pxPerSec;
            const width = Math.max(10, ((cue.endMs - cue.beginMs) / 1000) * pxPerSec);
            const isHighlighted = cue.id === highlightedCueId;
            return (
              <div
                key={cue.id}
                className={cn(
                  "absolute flex h-7 cursor-grab items-center overflow-hidden rounded-[var(--radius-sm)] border px-1.5 shadow-[0_10px_24px_rgba(0,0,0,0.2)] active:cursor-grabbing",
                  isHighlighted
                    ? "border-[var(--color-accent)] bg-[var(--accent-soft-strong)]"
                    : "border-[var(--color-line-strong)] bg-[color-mix(in_srgb,var(--color-surface-strong)_92%,transparent)] hover:border-[var(--accent-ring)]",
                )}
                style={{ top: `${CUE_BLOCK_TOP}px`, left: `${left}px`, width: `${width}px` }}
                title={cue.text}
                onClick={(event) => {
                  event.stopPropagation();
                  selectCue(cue);
                }}
                onDoubleClick={(event) => {
                  event.stopPropagation();
                  selectCue(cue);
                  focusCueEditor(cue.id);
                }}
                onPointerDown={(event) => onCuePointerDown(event, cue, "move")}
              >
                <span className="pointer-events-none truncate text-[11px] leading-none text-[var(--color-fg-muted)]">
                  {cue.text || "（空）"}
                </span>
                <span
                  className="absolute inset-y-0 left-0 w-2 cursor-ew-resize bg-transparent hover:bg-[var(--accent-ring)]"
                  onPointerDown={(event) => onCuePointerDown(event, cue, "left")}
                />
                <span
                  className="absolute inset-y-0 right-0 w-2 cursor-ew-resize bg-transparent hover:bg-[var(--accent-ring)]"
                  onPointerDown={(event) => onCuePointerDown(event, cue, "right")}
                />
              </div>
            );
          })}

          <span
            className="pointer-events-none absolute inset-y-0 w-px bg-[var(--color-accent)]"
            style={{
              left: `${playheadLeft}px`,
              boxShadow: "0 0 18px color-mix(in srgb, var(--color-accent) 70%, transparent)",
            }}
            aria-hidden
          />
        </div>
      </div>

      <div ref={listRef} className="max-h-[30rem] overflow-y-auto">
        {cues.length === 0 && <p className="px-4 py-6 text-sm text-[var(--color-fg-subtle)]">没有字幕条目。</p>}
        {cues.map((cue, index) => {
          const cueIndex = index;
          const isHighlighted = cue.id === highlightedCueId;
          return (
            <div
              key={cue.id}
              data-cue-id={cue.id}
              className={cn(
                "border-b border-[var(--color-line)] px-4 py-2.5 transition-colors duration-[var(--dur-fast)] last:border-b-0",
                isHighlighted && "bg-[var(--accent-soft)]",
              )}
              onClick={(event) => {
                if (isInteractiveTarget(event.target)) return;
                selectCue(cue);
              }}
              onDoubleClick={(event) => {
                if (isInteractiveTarget(event.target)) return;
                selectCue(cue);
                focusCueEditor(cue.id);
              }}
            >
              <div className="flex flex-wrap items-center gap-x-2 gap-y-1.5">
                <button
                  type="button"
                  title="跳转到该条开始位置"
                  onClick={(event) => {
                    event.stopPropagation();
                    selectCue(cue);
                  }}
                  className={cn(
                    "flex h-7 min-w-7 items-center justify-center rounded-[var(--radius-sm)] px-1 font-mono text-xs tabular-nums",
                    "transition-colors duration-[var(--dur-fast)] focus:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent-ring)]",
                    isHighlighted
                      ? "bg-[var(--color-accent)] text-[var(--color-accent-contrast)]"
                      : "text-[var(--color-fg-faint)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-fg)]",
                  )}
                >
                  {index + 1}
                </button>
                {cue.badge && (
                  <span
                    title={cue.badge.title}
                    className={cn(
                      "rounded-[var(--radius-pill)] px-1.5 py-0.5 text-[10px] leading-none",
                      cue.badge.tone === "warn"
                        ? "bg-[color-mix(in_srgb,var(--color-warn)_16%,transparent)] text-[var(--color-warn)]"
                        : "bg-[var(--accent-soft-strong)] text-[var(--color-accent-light)]",
                    )}
                  >
                    {cue.badge.label}
                  </span>
                )}
                <TimeControl
                  label="开始"
                  valueMs={cue.beginMs}
                  onCommit={(ms) => resizeCueLeft(cueIndex, ms)}
                  onSetPlayhead={() => resizeCueLeft(cueIndex, currentMs)}
                />
                <span className="text-xs text-[var(--color-fg-faint)]">→</span>
                <TimeControl
                  label="结束"
                  valueMs={cue.endMs}
                  onCommit={(ms) => resizeCueRight(cueIndex, ms)}
                  onSetPlayhead={() => resizeCueRight(cueIndex, currentMs)}
                />
                <span className="flex-1" />
                <span className="flex items-center gap-0.5">
                  <button type="button" title="在播放头（或中点）拆分该条" className={rowIconButton} onClick={(event) => {
                    event.stopPropagation();
                    splitCue(cue);
                  }}>
                    <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth={1.4} strokeLinecap="round" className="h-3.5 w-3.5" aria-hidden>
                      <path d="M8 2v12" strokeDasharray="2 2" />
                      <path d="M3 4.5 6 8l-3 3.5M13 4.5 10 8l3 3.5" />
                    </svg>
                  </button>
                  <button
                    type="button"
                    title="与下一条合并"
                    className={rowIconButton}
                    disabled={index === cues.length - 1}
                    onClick={(event) => {
                      event.stopPropagation();
                      mergeWithNext(cue);
                    }}
                  >
                    <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth={1.4} strokeLinecap="round" strokeLinejoin="round" className="h-3.5 w-3.5" aria-hidden>
                      <path d="M8 3v4M8 13V9M5 5.5 8 8l3-2.5M5 10.5 8 8l3 2.5" />
                    </svg>
                  </button>
                  <button type="button" title="在下方插入一条" className={rowIconButton} onClick={(event) => {
                    event.stopPropagation();
                    insertAfter(cue);
                  }}>
                    <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth={1.4} strokeLinecap="round" className="h-3.5 w-3.5" aria-hidden>
                      <path d="M8 3.5v9M3.5 8h9" />
                    </svg>
                  </button>
                  <button
                    type="button"
                    title="删除该条"
                    className={cn(rowIconButton, "hover:text-[var(--color-err)]")}
                    onClick={(event) => {
                      event.stopPropagation();
                      deleteCue(cue.id);
                    }}
                  >
                    <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth={1.4} strokeLinecap="round" className="h-3.5 w-3.5" aria-hidden>
                      <path d="M3 4.5h10M6.5 4V3h3v1M5 4.5l.6 8.5h4.8l.6-8.5" />
                    </svg>
                  </button>
                </span>
              </div>
              <CueTextarea
                value={cue.text}
                onChange={(text) => updateCueText(cue.id, { text })}
                textareaRef={(node) => {
                  cueTextRefs.current[cue.id] = node;
                }}
              />
            </div>
          );
        })}
      </div>

      <div className="flex flex-wrap items-center justify-between gap-3 border-t border-[var(--color-line)] px-4 py-2.5">
        <span className="text-xs text-[var(--color-fg-subtle)]">共 {cues.length} 条字幕</span>
        {footer && <span className="flex flex-wrap items-center gap-2">{footer}</span>}
      </div>
    </div>
  );
}
