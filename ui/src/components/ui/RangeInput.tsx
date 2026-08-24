import type { CSSProperties } from "react";
import { cn } from "@/lib/cn";

/** 扁平化范围滑块：统一轨道、进度和滑块视觉。 */
export function RangeInput({
  value,
  min,
  max,
  step,
  onChange,
  disabled = false,
  ariaLabel,
  className,
}: {
  value: number;
  min: number;
  max: number;
  step: number;
  onChange: (value: number) => void;
  disabled?: boolean;
  ariaLabel: string;
  className?: string;
}) {
  const progress = max === min ? 0 : Math.max(0, Math.min(100, ((value - min) / (max - min)) * 100));
  return (
    <div
      className={cn("range-control", className)}
      style={{ "--range-progress": `${progress}%` } as CSSProperties}
      data-disabled={disabled || undefined}
    >
      <input
        className="range-control-input"
        type="range"
        min={min}
        max={max}
        step={step}
        value={value}
        disabled={disabled}
        aria-label={ariaLabel}
        onChange={(event) => onChange(Number(event.target.value))}
      />
      <span className="range-control-visual" aria-hidden="true">
        <span className="range-control-track"><span className="range-control-fill" /></span>
        <span className="range-control-thumb" />
      </span>
    </div>
  );
}
