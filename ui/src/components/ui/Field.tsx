import { cn } from "@/lib/cn";
import { Checkbox } from "./Checkbox";

/**
 * 表单字段。
 * - layout="stack"（默认）：标签在上、控件在下，适合信息密度低的页面。
 * - layout="row"：标签在左、控件在右，适合高密度设置面板（如实时字幕基础设置）。
 */
export function Field({
  label,
  hint,
  actions,
  controlId,
  className,
  layout = "stack",
  children,
}: {
  label?: React.ReactNode;
  hint?: React.ReactNode;
  actions?: React.ReactNode;
  controlId?: string;
  className?: string;
  layout?: "stack" | "row";
  children: React.ReactNode;
}) {
  if (layout === "row") {
    return (
      <div className={cn("grid grid-cols-[5.5rem_minmax(0,1fr)] items-center gap-x-3 gap-y-1.5", className)}>
        {label && controlId ? (
          <label htmlFor={controlId} className="text-xs font-medium text-[var(--color-fg-muted)]">{label}</label>
        ) : label ? (
          <span className="text-xs font-medium text-[var(--color-fg-muted)]">{label}</span>
        ) : null}
        <div className="flex min-w-0 items-stretch gap-2">
          <div className="min-w-0 flex-1">{children}</div>
          {actions && <div className="flex shrink-0 items-stretch gap-2">{actions}</div>}
        </div>
        {hint && (
          <span className="col-start-2 text-xs text-[var(--color-fg-subtle)]">{hint}</span>
        )}
      </div>
    );
  }

  if (actions || controlId) {
    return (
      <div className={cn("flex flex-col gap-1.5", className)}>
        {label && controlId ? (
          <label htmlFor={controlId} className="text-xs font-medium text-[var(--color-fg-muted)]">{label}</label>
        ) : label ? (
          <span className="text-xs font-medium text-[var(--color-fg-muted)]">{label}</span>
        ) : null}
        <div className="flex min-w-0 items-stretch gap-2">
          <div className="min-w-0 flex-1">{children}</div>
          {actions && <div className="flex shrink-0 items-stretch gap-2">{actions}</div>}
        </div>
        {hint && <span className="text-xs text-[var(--color-fg-subtle)]">{hint}</span>}
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
      <Checkbox
        checked={checked}
        disabled={disabled}
        onChange={(e) => onChange(e.target.checked)}
      />
      {children}
    </label>
  );
}
