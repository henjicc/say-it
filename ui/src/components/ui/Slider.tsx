import { cn } from "@/lib/cn";

/** 参数滑块：标签 + range + 数值。 */
export function Slider({
  label,
  value,
  min,
  max,
  step,
  onChange,
  format,
  className,
}: {
  label: React.ReactNode;
  value: number;
  min: number;
  max: number;
  step: number;
  onChange: (value: number) => void;
  format?: (value: number) => string;
  className?: string;
}) {
  return (
    <div className={cn("grid grid-cols-[7rem_1fr_3.5rem] items-center gap-3", className)}>
      <span className="text-xs text-[var(--color-fg-muted)]">{label}</span>
      <input
        type="range"
        min={min}
        max={max}
        step={step}
        value={value}
        onChange={(e) => onChange(parseFloat(e.target.value))}
        className="h-1.5 w-full cursor-pointer appearance-none rounded-full bg-[var(--color-surface-strong)] [accent-color:var(--color-accent)]"
      />
      <span className="text-right text-xs tabular-nums text-[var(--color-fg-muted)]">
        {format ? format(value) : value}
      </span>
    </div>
  );
}
