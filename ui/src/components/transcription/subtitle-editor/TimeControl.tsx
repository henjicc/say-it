import { LocateFixed, Minus, Plus } from "lucide-react";
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
        <LocateFixed className="h-3.5 w-3.5" strokeWidth={1.5} aria-hidden />
      </button>
      <button type="button" title={`${label} -0.1 秒`} className={iconButton} onClick={() => onCommit(Math.max(0, valueMs - NUDGE_MS))}>
        <Minus className="h-3.5 w-3.5" strokeWidth={1.5} aria-hidden />
      </button>
      <button type="button" title={`${label} +0.1 秒`} className={iconButton} onClick={() => onCommit(valueMs + NUDGE_MS)}>
        <Plus className="h-3.5 w-3.5" strokeWidth={1.5} aria-hidden />
      </button>
    </span>
  );
}
