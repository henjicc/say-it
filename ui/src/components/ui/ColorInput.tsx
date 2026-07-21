import { cn } from "@/lib/cn";

/**
 * 颜色选择器色板：与其它表单控件同高（--control-h），方形。
 *
 * 原本在「外观」和「字幕样式」各有一份原生 input[type=color]，高度 40 / 44 两种、
 * 边框和内边距也不一致。同一个控件只保留这一份，尺寸走令牌而不是 h-10 / h-11。
 */
export function ColorInput({
  value,
  onChange,
  label,
  className,
}: {
  value: string;
  onChange: (value: string) => void;
  label: string;
  className?: string;
}) {
  return (
    <input
      type="color"
      value={value}
      onChange={(event) => onChange(event.target.value)}
      aria-label={label}
      className={cn(
        "h-[var(--control-h)] w-[var(--control-h)] flex-none cursor-pointer rounded-[var(--radius-md)]",
        "border border-[var(--color-line)] bg-[var(--color-surface)] p-1",
        "focus:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent-ring)]",
        className,
      )}
    />
  );
}
