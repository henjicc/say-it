import { cn } from "@/lib/cn";

export type StatusTone = "idle" | "info" | "running" | "ok" | "err";

const dotClass: Record<StatusTone, string> = {
  idle: "bg-[var(--color-fg-faint)]",
  info: "bg-[var(--color-accent)]",
  running: "bg-[var(--color-accent)] animate-pulse",
  ok: "bg-[var(--color-ok)]",
  err: "bg-[var(--color-err)]",
};

const textClass: Record<StatusTone, string> = {
  idle: "text-[var(--color-fg-muted)]",
  info: "text-[var(--color-fg-muted)]",
  running: "text-[var(--color-fg-muted)]",
  ok: "text-[var(--color-ok)]",
  err: "text-[var(--color-err)]",
};

/**
 * 状态条：圆点 + 主状态文案，可附加补充说明行。
 * - variant="panel"（默认）：带边框底色的区块，用于结果/流程状态。
 * - variant="inline"：无边框，用于页头下方的轻量状态提示。
 */
export function StatusBar({
  tone = "idle",
  message,
  children,
  variant = "panel",
  className,
}: {
  tone?: StatusTone;
  message: React.ReactNode;
  children?: React.ReactNode;
  variant?: "panel" | "inline";
  className?: string;
}) {
  return (
    <div
      className={cn(
        "flex flex-col gap-2",
        variant === "panel" &&
          "rounded-[var(--radius-lg)] border border-[var(--color-line)] bg-[var(--color-surface)] px-4 py-3.5",
        className,
      )}
    >
      <div className="flex items-center gap-2.5">
        <span className={cn("h-2.5 w-2.5 flex-none rounded-full", dotClass[tone])} aria-hidden />
        <p className={cn("text-sm", textClass[tone])}>{message}</p>
      </div>
      {children}
    </div>
  );
}
