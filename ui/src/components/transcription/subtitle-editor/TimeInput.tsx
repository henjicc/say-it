import { useState } from "react";
import { cn } from "@/lib/cn";
import { formatClock, parseClock } from "@/features/transcription/subtitles";

export function TimeInput({
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
