import { cn } from "@/lib/cn";

type Tone = "ok" | "err" | "warn" | "idle" | "rec";

const tones: Record<Tone, string> = {
  ok: "bg-[#25c36f]",
  err: "bg-[#ff6b6b]",
  warn: "bg-[#ffd166]",
  idle: "bg-white/30",
  rec: "bg-[#ff4d4f]",
};

export function StatusDot({ tone = "idle", className }: { tone?: Tone; className?: string }) {
  return (
    <span
      className={cn("inline-block h-2.5 w-2.5 flex-none rounded-full", tones[tone], className)}
    />
  );
}
