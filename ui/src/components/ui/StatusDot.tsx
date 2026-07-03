import { cn } from "@/lib/cn";

type Tone = "ok" | "err" | "warn" | "idle" | "rec";

const tones: Record<Tone, string> = {
  ok: "bg-[var(--color-ok)]",
  err: "bg-[var(--color-err)]",
  warn: "bg-[var(--color-warn)]",
  idle: "bg-[var(--color-fg-faint)]",
  rec: "bg-[var(--color-rec)]",
};

export function StatusDot({ tone = "idle", className }: { tone?: Tone; className?: string }) {
  return (
    <span
      className={cn("inline-block h-2.5 w-2.5 flex-none rounded-full", tones[tone], className)}
    />
  );
}
