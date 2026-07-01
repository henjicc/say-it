import { cn } from "@/lib/cn";

/** 表单字段：标签在上、控件在下。 */
export function Field({
  label,
  hint,
  className,
  children,
}: {
  label?: React.ReactNode;
  hint?: React.ReactNode;
  className?: string;
  children: React.ReactNode;
}) {
  return (
    <label className={cn("flex flex-col gap-1.5", className)}>
      {label && <span className="text-xs font-medium text-white/60">{label}</span>}
      {children}
      {hint && <span className="text-xs text-white/40">{hint}</span>}
    </label>
  );
}

/** 横排复选项：复选框 + 文案。 */
export function CheckField({
  checked,
  onChange,
  children,
  className,
  disabled,
}: {
  checked: boolean;
  onChange: (checked: boolean) => void;
  children: React.ReactNode;
  className?: string;
  disabled?: boolean;
}) {
  return (
    <label
      className={cn(
        "inline-flex cursor-pointer items-center gap-2.5 text-sm text-white/80 select-none",
        disabled && "cursor-not-allowed opacity-50",
        className,
      )}
    >
      <input
        type="checkbox"
        checked={checked}
        disabled={disabled}
        onChange={(e) => onChange(e.target.checked)}
        className="h-4 w-4 [accent-color:var(--color-accent)]"
      />
      {children}
    </label>
  );
}
