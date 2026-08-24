import { cn } from "@/lib/cn";
import { RangeInput } from "@/components/ui/RangeInput";

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
  disabled = false,
}: {
  label: React.ReactNode;
  value: number;
  min: number;
  max: number;
  step: number;
  onChange: (value: number) => void;
  format?: (value: number) => string;
  className?: string;
  disabled?: boolean;
}) {
  return (
    <div className={cn("grid grid-cols-[7rem_1fr_3.5rem] items-center gap-3", disabled && "opacity-50", className)}>
      <span className="text-xs text-[var(--color-fg-muted)]">{label}</span>
      <RangeInput
        ariaLabel={typeof label === "string" ? label : "调整参数"}
        min={min}
        max={max}
        step={step}
        value={value}
        disabled={disabled}
        onChange={onChange}
      />
      <span className="text-right text-xs tabular-nums text-[var(--color-fg-muted)]">
        {format ? format(value) : value}
      </span>
    </div>
  );
}
