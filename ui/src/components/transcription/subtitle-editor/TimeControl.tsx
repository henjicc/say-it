import { NUDGE_MS } from "./constants";
import { TimeInput } from "./TimeInput";

export function TimeControl({
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
