import { useEffect, useMemo, useRef, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { Button } from "@/components/ui/Button";
import { cn } from "@/lib/cn";
import {
  formatClock,
  parseClock,
  type EditableCue,
} from "@/features/transcription/subtitles";

/** 时间轴缩放：每秒对应的像素宽度。 */
const BASE_PX_PER_SEC = 60;
/** 字幕条目的最短时长（毫秒），拖拽/输入时间时的下限。 */
const MIN_CUE_MS = 100;
/** 时间微调步长（毫秒）。 */
const NUDGE_MS = 100;
const RATE_OPTIONS = [0.75, 1, 1.25, 1.5];
const ZOOM_LEVELS = [0.5, 0.75, 1, 1.5, 2, 3];
const WAVEFORM_HEIGHT = 48;
const WAVEFORM_VERTICAL_PADDING = 6;
const MIN_WAVEFORM_BUCKETS = 120;
const MAX_WAVEFORM_BUCKETS = 2400;

type DragMode = "move" | "left" | "right";

interface DragState {
  id: string;
  mode: DragMode;
  startX: number;
  beginMs: number;
  endMs: number;
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
  return tagName === "INPUT" || tagName === "TEXTAREA" || tagName === "SELECT" || tagName === "BUTTON";
}

function buildWaveformPeaks(buffer: AudioBuffer, bucketCount: number) {
  const channelCount = Math.max(1, buffer.numberOfChannels);
  const channels = Array.from({ length: channelCount }, (_, index) => buffer.getChannelData(index));
  const sampleCount = channels[0]?.length || 0;
  if (sampleCount === 0) return [];

  const safeBucketCount = Math.max(1, Math.min(bucketCount, sampleCount));
  const samplesPerBucket = Math.max(1, Math.floor(sampleCount / safeBucketCount));
  const peaks = new Array<number>(safeBucketCount).fill(0);
  let globalMax = 0;

  for (let bucketIndex = 0; bucketIndex < safeBucketCount; bucketIndex += 1) {
    const start = bucketIndex * samplesPerBucket;
    const end = bucketIndex === safeBucketCount - 1 ? sampleCount : Math.min(sampleCount, start + samplesPerBucket);
    let peak = 0;
    for (let sampleIndex = start; sampleIndex < end; sampleIndex += 1) {
      for (const channel of channels) {
        const value = Math.abs(channel[sampleIndex] || 0);
        if (value > peak) peak = value;
      }
    }
    peaks[bucketIndex] = peak;
    if (peak > globalMax) globalMax = peak;
  }

  if (globalMax <= 0) return peaks.map(() => 0);
  return peaks.map((peak) => Math.min(1, Math.sqrt(peak / globalMax)));
}

/** 合并两段文本：英文/数字相接时补空格，中文直接拼接。 */
function joinTexts(a: string, b: string) {
  const left = a.trimEnd();
  const right = b.trimStart();
  if (!left) return right;
  if (!right) return left;
  return /[a-zA-Z0-9]$/.test(left) && /^[a-zA-Z0-9]/.test(right) ? `${left} ${right}` : `${left}${right}`;
}

/** 起止时间输入框：失焦/回车提交，Escape 或非法输入还原。 */
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

/** 随内容自适应高度的字幕文本框。 */
function CueTextarea({
  value,
  onChange,
}: {
  value: string;
  onChange: (text: string) => void;
}) {
  const ref = useRef<HTMLTextAreaElement>(null);
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${el.scrollHeight}px`;
  }, [value]);
  return (
    <textarea
      ref={ref}
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

/** 单侧时间控件：输入框 + 设为播放头 + ±0.1s 微调。 */
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

/**
 * 字幕编辑器：音频同步播放 + 时间轴拖拽 + 逐条编辑。
 * 操作的是工作副本 cue 数组，所有修改通过 onCuesChange 上报；
 * WebView 无法解码的容器（mkv/wmv 等）会降级为纯编辑模式，播放不可用。
 */
export function SubtitleEditor({
  mediaPath,
  cues,
  onCuesChange,
  footer,
}: {
  mediaPath: string | null;
  cues: EditableCue[];
  onCuesChange: (next: EditableCue[]) => void;
  /** 底部右侧的操作区（导出/复制等），由调用方提供。 */
  footer?: React.ReactNode;
}) {
  const audioRef = useRef<HTMLAudioElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const timelineRef = useRef<HTMLDivElement>(null);
  const dragRef = useRef<DragState | null>(null);
  const renderFrameRef = useRef(0);
  const [playing, setPlaying] = useState(false);
  const [currentMs, setCurrentMs] = useState(0);
  const [durationMs, setDurationMs] = useState(0);
  const [rate, setRate] = useState(1);
  const [zoomScale, setZoomScale] = useState(1);
  const [playbackError, setPlaybackError] = useState(false);
  const [waveformPeaks, setWaveformPeaks] = useState<number[]>([]);
  const [waveformLoading, setWaveformLoading] = useState(false);

  const mediaSrc = useMemo(() => (mediaPath ? convertFileSrc(mediaPath) : ""), [mediaPath]);
  const lastEndMs = useMemo(() => cues.reduce((max, cue) => Math.max(max, cue.endMs), 0), [cues]);
  const totalMs = Math.max(durationMs, lastEndMs);
  const pxPerSec = BASE_PX_PER_SEC * zoomScale;
  const playheadLeft = (currentMs / 1000) * pxPerSec;
  const timelineWidth = Math.max(320, Math.ceil((totalMs / 1000) * pxPerSec) + 80);
  const zoomIndex = ZOOM_LEVELS.indexOf(zoomScale);
  const activeCueId = useMemo(() => {
    const active = cues.find((cue) => currentMs >= cue.beginMs && currentMs < cue.endMs);
    return active?.id ?? null;
  }, [cues, currentMs]);
  const waveformPath = useMemo(() => {
    if (waveformPeaks.length === 0) return "";
    const width = Math.max(1, timelineWidth);
    const centerY = WAVEFORM_HEIGHT / 2;
    const amplitude = centerY - WAVEFORM_VERTICAL_PADDING;
    if (waveformPeaks.length === 1) {
      const y = centerY - waveformPeaks[0] * amplitude;
      return `M 0 ${centerY} L 0 ${y} L ${width} ${y} L ${width} ${centerY} Z`;
    }
    const top = waveformPeaks
      .map((peak, index) => {
        const x = (index / (waveformPeaks.length - 1)) * width;
        const y = centerY - peak * amplitude;
        return `${x.toFixed(2)},${y.toFixed(2)}`;
      })
      .join(" ");
    const bottom = waveformPeaks
      .map((peak, index) => {
        const reverseIndex = waveformPeaks.length - 1 - index;
        const x = (reverseIndex / (waveformPeaks.length - 1)) * width;
        const y = centerY + waveformPeaks[reverseIndex] * amplitude;
        return `${x.toFixed(2)},${y.toFixed(2)}`;
      })
      .join(" ");
    return `M 0 ${centerY} L ${top} L ${bottom} Z`;
  }, [timelineWidth, waveformPeaks]);

  const syncPlayhead = () => {
    const audio = audioRef.current;
    if (!audio) return;
    setCurrentMs(audio.currentTime * 1000);
    if (!audio.paused && !audio.ended) {
      renderFrameRef.current = requestAnimationFrame(syncPlayhead);
    } else {
      renderFrameRef.current = 0;
    }
  };

  const stopPlayheadSync = () => {
    if (!renderFrameRef.current) return;
    cancelAnimationFrame(renderFrameRef.current);
    renderFrameRef.current = 0;
  };

  // 切换媒体文件时重置播放状态
  useEffect(() => {
    stopPlayheadSync();
    setPlaying(false);
    setCurrentMs(0);
    setDurationMs(0);
    setPlaybackError(false);
    setWaveformPeaks([]);
    setWaveformLoading(false);
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
              Math.round((buffer.duration * BASE_PX_PER_SEC * ZOOM_LEVELS[ZOOM_LEVELS.length - 1]) / 4),
            ),
          );
          setWaveformPeaks(buildWaveformPeaks(buffer, bucketCount));
          setDurationMs((current) => current || Math.round(buffer.duration * 1000));
        } finally {
          void audioContext.close().catch(() => {});
        }
      } catch {
        if (!disposed) setWaveformPeaks([]);
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
    const audio = audioRef.current;
    if (audio) audio.playbackRate = rate;
  }, [rate]);

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

  // 播放中让当前字幕行保持可见
  useEffect(() => {
    if (!playing || !activeCueId) return;
    listRef.current
      ?.querySelector(`[data-cue-id="${activeCueId}"]`)
      ?.scrollIntoView({ block: "nearest", behavior: "smooth" });
  }, [playing, activeCueId]);

  // 播放中让时间轴播放头保持可见
  useEffect(() => {
    if (!playing) return;
    const container = timelineRef.current;
    if (!container) return;
    if (playheadLeft < container.scrollLeft + 40 || playheadLeft > container.scrollLeft + container.clientWidth - 80) {
      container.scrollLeft = Math.max(0, playheadLeft - container.clientWidth / 3);
    }
  }, [playing, playheadLeft]);

  const seek = (ms: number) => {
    const target = Math.max(0, totalMs > 0 ? Math.min(ms, totalMs) : ms);
    setCurrentMs(target);
    const audio = audioRef.current;
    if (audio && !playbackError && Number.isFinite(audio.duration)) {
      audio.currentTime = target / 1000;
    }
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

  const cycleRate = () => {
    const next = RATE_OPTIONS[(RATE_OPTIONS.indexOf(rate) + 1) % RATE_OPTIONS.length];
    setRate(next);
    if (audioRef.current) audioRef.current.playbackRate = next;
  };

  const zoomOut = () => {
    if (zoomIndex <= 0) return;
    setZoomScale(ZOOM_LEVELS[zoomIndex - 1]);
  };

  const zoomIn = () => {
    if (zoomIndex >= ZOOM_LEVELS.length - 1) return;
    setZoomScale(ZOOM_LEVELS[zoomIndex + 1]);
  };

  const updateCue = (id: string, patch: Partial<EditableCue>) => {
    onCuesChange(cues.map((cue) => (cue.id === id ? { ...cue, ...patch } : cue)));
  };

  const commitBegin = (cue: EditableCue, ms: number) => {
    updateCue(cue.id, { beginMs: clamp(ms, 0, cue.endMs - MIN_CUE_MS) });
  };

  const commitEnd = (cue: EditableCue, ms: number) => {
    updateCue(cue.id, { endMs: Math.max(cue.beginMs + MIN_CUE_MS, ms) });
  };

  const deleteCue = (id: string) => {
    onCuesChange(cues.filter((cue) => cue.id !== id));
  };

  const splitCue = (cue: EditableCue) => {
    // 播放头在条目内则按播放头拆，否则从正中间拆；文本按时间比例分配
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
    const index = cues.findIndex((item) => item.id === cue.id);
    onCuesChange([...cues.slice(0, index), first, second, ...cues.slice(index + 1)]);
  };

  const mergeWithNext = (cue: EditableCue) => {
    const index = cues.findIndex((item) => item.id === cue.id);
    const next = cues[index + 1];
    if (!next) return;
    const merged: EditableCue = {
      ...cue,
      endMs: Math.max(cue.endMs, next.endMs),
      text: joinTexts(cue.text, next.text),
    };
    onCuesChange([...cues.slice(0, index), merged, ...cues.slice(index + 2)]);
  };

  const insertAfter = (cue: EditableCue) => {
    const index = cues.findIndex((item) => item.id === cue.id);
    const next = cues[index + 1];
    const beginMs = cue.endMs;
    const endMs = next ? Math.max(beginMs + MIN_CUE_MS, Math.min(next.beginMs, beginMs + 2000)) : beginMs + 2000;
    const inserted: EditableCue = {
      id: `${cue.id}-i${Date.now().toString(36)}`,
      beginMs,
      endMs,
      text: "",
    };
    onCuesChange([...cues.slice(0, index + 1), inserted, ...cues.slice(index + 1)]);
  };

  // ---- 时间轴拖拽 ----
  const onBlockPointerDown = (event: React.PointerEvent, cue: EditableCue, mode: DragMode) => {
    if (event.button !== 0) return;
    event.preventDefault();
    event.stopPropagation();
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    dragRef.current = { id: cue.id, mode, startX: event.clientX, beginMs: cue.beginMs, endMs: cue.endMs };
  };

  const onBlockPointerMove = (event: React.PointerEvent) => {
    const drag = dragRef.current;
    if (!drag) return;
    const deltaMs = ((event.clientX - drag.startX) / pxPerSec) * 1000;
    const duration = drag.endMs - drag.beginMs;
    if (drag.mode === "move") {
      const beginMs = Math.max(0, Math.round(drag.beginMs + deltaMs));
      updateCue(drag.id, { beginMs, endMs: beginMs + duration });
    } else if (drag.mode === "left") {
      updateCue(drag.id, { beginMs: clamp(Math.round(drag.beginMs + deltaMs), 0, drag.endMs - MIN_CUE_MS) });
    } else {
      updateCue(drag.id, { endMs: Math.max(drag.beginMs + MIN_CUE_MS, Math.round(drag.endMs + deltaMs)) });
    }
  };

  const onBlockPointerUp = () => {
    dragRef.current = null;
  };

  const onTimelinePointerDown = (event: React.PointerEvent<HTMLDivElement>) => {
    const container = timelineRef.current;
    if (!container) return;
    const rect = container.getBoundingClientRect();
    const x = event.clientX - rect.left + container.scrollLeft;
    seek((x / pxPerSec) * 1000);
  };

  const secondLabels = useMemo(() => {
    const labels: number[] = [];
    for (let s = 0; s <= Math.ceil(totalMs / 1000); s += 10) labels.push(s);
    return labels;
  }, [totalMs]);

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

      {/* 播放控制条 */}
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
        <span className="inline-flex items-center gap-1 rounded-[var(--radius-pill)] border border-[var(--color-line)] px-1 py-1">
          <button
            type="button"
            onClick={zoomOut}
            disabled={zoomIndex <= 0}
            title="缩小波形与时间轴"
            className="flex h-6 w-6 items-center justify-center rounded-[var(--radius-sm)] text-sm text-[var(--color-fg-subtle)] transition-colors hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-fg)] disabled:opacity-35"
          >
            −
          </button>
          <button
            type="button"
            onClick={() => setZoomScale(1)}
            title="重置缩放"
            className="min-w-12 rounded-[var(--radius-sm)] px-1.5 py-0.5 font-mono text-[11px] text-[var(--color-fg-faint)] transition-colors hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-fg)]"
          >
            {formatZoom(zoomScale)}
          </button>
          <button
            type="button"
            onClick={zoomIn}
            disabled={zoomIndex >= ZOOM_LEVELS.length - 1}
            title="放大波形与时间轴"
            className="flex h-6 w-6 items-center justify-center rounded-[var(--radius-sm)] text-sm text-[var(--color-fg-subtle)] transition-colors hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-fg)] disabled:opacity-35"
          >
            +
          </button>
        </span>
        <span className="rounded-[var(--radius-pill)] border border-[var(--color-line)] px-2 py-1 font-mono text-[11px] leading-none text-[var(--color-fg-faint)]">
          Space 播放/暂停
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

      {/* 时间轴：标尺 + 可拖拽字幕块 + 播放头 */}
      <div
        ref={timelineRef}
        className="relative overflow-x-auto border-b border-[var(--color-line)] bg-[var(--color-bg)]"
        onPointerDown={onTimelinePointerDown}
      >
        <div className="relative h-24 select-none" style={{ width: `${timelineWidth}px` }}>
          {/* 秒刻度：细线由渐变生成，数字每 10 秒一个 */}
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

          <div className="pointer-events-none absolute inset-x-0 top-6 h-12 overflow-hidden rounded-[var(--radius-md)] border border-[color-mix(in_srgb,var(--color-line)_72%,transparent)] bg-[linear-gradient(180deg,color-mix(in_srgb,var(--color-surface)_70%,transparent),color-mix(in_srgb,var(--color-bg)_92%,transparent))]">
            {waveformPath ? (
              <svg
                width={timelineWidth}
                height={WAVEFORM_HEIGHT}
                viewBox={`0 0 ${timelineWidth} ${WAVEFORM_HEIGHT}`}
                preserveAspectRatio="none"
                className="absolute inset-0 h-full w-full"
                aria-hidden
              >
                <path
                  d={waveformPath}
                  fill="color-mix(in srgb, var(--color-accent) 24%, transparent)"
                  stroke="color-mix(in srgb, var(--color-accent-light) 55%, transparent)"
                  strokeWidth="1"
                />
              </svg>
            ) : (
              <div className="flex h-full items-center justify-center text-[10px] text-[var(--color-fg-faint)]">
                {waveformLoading ? "正在生成波形…" : "当前文件暂不支持波形预览"}
              </div>
            )}
          </div>

          {/* 字幕块 */}
          {cues.map((cue) => {
            const left = (cue.beginMs / 1000) * pxPerSec;
            const width = Math.max(8, ((cue.endMs - cue.beginMs) / 1000) * pxPerSec);
            const isActive = cue.id === activeCueId;
            return (
              <div
                key={cue.id}
                className={cn(
                  "absolute top-9 flex h-10 cursor-grab items-center overflow-hidden rounded-[var(--radius-sm)] border px-1.5 shadow-[0_10px_24px_rgba(0,0,0,0.24)] active:cursor-grabbing",
                  isActive
                    ? "border-[var(--color-accent)] bg-[var(--accent-soft-strong)]"
                    : "border-[var(--color-line-strong)] bg-[var(--color-surface-strong)] hover:border-[var(--accent-ring)]",
                )}
                style={{ left: `${left}px`, width: `${width}px` }}
                title={cue.text}
                onPointerDown={(event) => onBlockPointerDown(event, cue, "move")}
                onPointerMove={onBlockPointerMove}
                onPointerUp={onBlockPointerUp}
              >
                <span className="pointer-events-none truncate text-[11px] leading-none text-[var(--color-fg-muted)]">
                  {cue.text || "（空）"}
                </span>
                <span
                  className="absolute inset-y-0 left-0 w-1.5 cursor-ew-resize bg-transparent hover:bg-[var(--accent-ring)]"
                  onPointerDown={(event) => onBlockPointerDown(event, cue, "left")}
                  onPointerMove={onBlockPointerMove}
                  onPointerUp={onBlockPointerUp}
                />
                <span
                  className="absolute inset-y-0 right-0 w-1.5 cursor-ew-resize bg-transparent hover:bg-[var(--accent-ring)]"
                  onPointerDown={(event) => onBlockPointerDown(event, cue, "right")}
                  onPointerMove={onBlockPointerMove}
                  onPointerUp={onBlockPointerUp}
                />
              </div>
            );
          })}

          {/* 播放头 */}
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

      {/* 逐条编辑列表 */}
      <div ref={listRef} className="max-h-[30rem] overflow-y-auto">
        {cues.length === 0 && <p className="px-4 py-6 text-sm text-[var(--color-fg-subtle)]">没有字幕条目。</p>}
        {cues.map((cue, index) => {
          const isActive = cue.id === activeCueId;
          return (
            <div
              key={cue.id}
              data-cue-id={cue.id}
              className={cn(
                "border-b border-[var(--color-line)] px-4 py-2.5 transition-colors duration-[var(--dur-fast)] last:border-b-0",
                isActive && "bg-[var(--accent-soft)]",
              )}
            >
              <div className="flex flex-wrap items-center gap-x-2 gap-y-1.5">
                <button
                  type="button"
                  title="跳转到该条开始位置"
                  onClick={() => seek(cue.beginMs)}
                  className={cn(
                    "flex h-7 min-w-7 items-center justify-center rounded-[var(--radius-sm)] px-1 font-mono text-xs tabular-nums",
                    "transition-colors duration-[var(--dur-fast)] focus:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent-ring)]",
                    isActive
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
                  onCommit={(ms) => commitBegin(cue, ms)}
                  onSetPlayhead={() => commitBegin(cue, currentMs)}
                />
                <span className="text-xs text-[var(--color-fg-faint)]">→</span>
                <TimeControl
                  label="结束"
                  valueMs={cue.endMs}
                  onCommit={(ms) => commitEnd(cue, ms)}
                  onSetPlayhead={() => commitEnd(cue, currentMs)}
                />
                <span className="flex-1" />
                <span className="flex items-center gap-0.5">
                  <button type="button" title="在播放头（或中点）拆分该条" className={rowIconButton} onClick={() => splitCue(cue)}>
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
                    onClick={() => mergeWithNext(cue)}
                  >
                    <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth={1.4} strokeLinecap="round" strokeLinejoin="round" className="h-3.5 w-3.5" aria-hidden>
                      <path d="M8 3v4M8 13V9M5 5.5 8 8l3-2.5M5 10.5 8 8l3 2.5" />
                    </svg>
                  </button>
                  <button type="button" title="在下方插入一条" className={rowIconButton} onClick={() => insertAfter(cue)}>
                    <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth={1.4} strokeLinecap="round" className="h-3.5 w-3.5" aria-hidden>
                      <path d="M8 3.5v9M3.5 8h9" />
                    </svg>
                  </button>
                  <button
                    type="button"
                    title="删除该条"
                    className={cn(rowIconButton, "hover:text-[var(--color-err)]")}
                    onClick={() => deleteCue(cue.id)}
                  >
                    <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth={1.4} strokeLinecap="round" className="h-3.5 w-3.5" aria-hidden>
                      <path d="M3 4.5h10M6.5 4V3h3v1M5 4.5l.6 8.5h4.8l.6-8.5" />
                    </svg>
                  </button>
                </span>
              </div>
              <CueTextarea value={cue.text} onChange={(text) => updateCue(cue.id, { text })} />
            </div>
          );
        })}
      </div>

      {/* 底部：统计 + 调用方操作 */}
      <div className="flex flex-wrap items-center justify-between gap-3 border-t border-[var(--color-line)] px-4 py-2.5">
        <span className="text-xs text-[var(--color-fg-subtle)]">共 {cues.length} 条字幕</span>
        {footer && <span className="flex flex-wrap items-center gap-2">{footer}</span>}
      </div>
    </div>
  );
}
