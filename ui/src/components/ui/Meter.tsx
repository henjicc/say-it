import { cn } from "@/lib/cn";

/** 无字的进度条或电平（`kind="level"`），强调色填充。`value` 取 0～1。 */
export function Meter({
  value,
  label,
  kind = "progress",
  className,
}: {
  value: number;
  label: string;
  kind?: "progress" | "level";
  className?: string;
}) {
  const percent = Math.round(Math.min(1, Math.max(0, value)) * 100);
  return (
    <div
      role={kind === "level" ? "meter" : "progressbar"}
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={percent}
      className={cn("h-1.5 overflow-hidden rounded-full bg-[var(--color-surface-strong)]", className)}
    >
      <span className="block h-full rounded-full bg-[var(--color-accent)] transition-[width] duration-75" style={{ width: `${percent}%` }} />
    </div>
  );
}
