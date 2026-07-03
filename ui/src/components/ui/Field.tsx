import { cn } from "@/lib/cn";

/**
 * 表单字段。
 * - layout="stack"（默认）：标签在上、控件在下，适合信息密度低的页面。
 * - layout="row"：标签在左、控件在右，适合高密度设置面板（如实时字幕基础设置）。
 */
export function Field({
  label,
  hint,
  className,
  layout = "stack",
  children,
}: {
  label?: React.ReactNode;
  hint?: React.ReactNode;
  className?: string;
  layout?: "stack" | "row";
  children: React.ReactNode;
}) {
  if (layout === "row") {
    return (
      <div className={cn("grid grid-cols-[5.5rem_minmax(0,1fr)] items-center gap-x-3 gap-y-1.5", className)}>
        {label && (
          <span className="text-xs font-medium text-[var(--color-fg-muted)]">{label}</span>
        )}
        <div className="min-w-0">{children}</div>
        {hint && (
          <span className="col-start-2 text-xs text-[var(--color-fg-subtle)]">{hint}</span>
        )}
      </div>
    );
  }

  return (
    <label className={cn("flex flex-col gap-1.5", className)}>
      {label && <span className="text-xs font-medium text-[var(--color-fg-muted)]">{label}</span>}
      {children}
      {hint && <span className="text-xs text-[var(--color-fg-subtle)]">{hint}</span>}
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
        "inline-flex cursor-pointer items-center gap-2.5 text-sm text-[var(--color-fg-muted)] select-none",
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
