import { cn } from "@/lib/cn";
import { Tooltip } from "./Tooltip";
import { Checkbox } from "./Checkbox";

/**
 * 操作区：始终与左侧控件同高。
 *
 * Input/Select 只有 --control-h 一种高度，而 Button/IconButton 的 size="sm" 会写死
 * --control-h-sm；固定高度压得过 items-stretch，所以光靠 flex 对齐是保证不了的。
 * 这里直接钉死高度，调用方传错 size 也不会矮一截——这类错位只有截图才看得出来，
 * 不能依赖每个页面作者记住「actions 里不要用 sm」。
 */
const ACTIONS_CLASS = "flex shrink-0 items-stretch gap-2 [&>*]:h-[var(--control-h)]!";

/**
 * 表单字段。
 * - layout="stack"（默认）：标签在上、控件在下，适合信息密度低的页面。
 * - layout="row"：标签在左、控件在右，适合高密度设置面板（如实时字幕基础设置）。
 */
export function Field({
  label,
  hint,
  message,
  actions,
  controlId,
  className,
  layout = "stack",
  children,
}: {
  label?: React.ReactNode;
  hint?: React.ReactNode;
  /** 必须立即可见的错误、风险或操作反馈。 */
  message?: React.ReactNode;
  actions?: React.ReactNode;
  controlId?: string;
  className?: string;
  layout?: "stack" | "row";
  children: React.ReactNode;
}) {
  const labelContent = hint ? <span tabIndex={0} className="ui-help-label">{label}</span> : label;
  if (layout === "row") {
    return (
      <Tooltip content={hint}><div className={cn("grid grid-cols-[5.5rem_minmax(0,1fr)] items-center gap-x-3 gap-y-1.5", className)}>
        {label && controlId ? (
          <label htmlFor={controlId} className="text-xs font-medium text-[var(--color-fg-muted)]">{labelContent}</label>
        ) : label ? (
          <span className="text-xs font-medium text-[var(--color-fg-muted)]">{labelContent}</span>
        ) : null}
        <div className="flex min-w-0 items-stretch gap-2">
          <div className="min-w-0 flex-1">{children}</div>
          {actions && <div className={ACTIONS_CLASS}>{actions}</div>}
        </div>
        {message && (
          <span className="col-start-2 text-xs text-[var(--color-fg-subtle)]" role="status">{message}</span>
        )}
      </div></Tooltip>
    );
  }

  if (actions || controlId) {
    return (
      <Tooltip content={hint}><div className={cn("flex flex-col gap-1.5", className)}>
        {label && controlId ? (
          <label htmlFor={controlId} className="text-xs font-medium text-[var(--color-fg-muted)]">{labelContent}</label>
        ) : label ? (
          <span className="text-xs font-medium text-[var(--color-fg-muted)]">{labelContent}</span>
        ) : null}
        <div className="flex min-w-0 items-stretch gap-2">
          <div className="min-w-0 flex-1">{children}</div>
          {actions && <div className={ACTIONS_CLASS}>{actions}</div>}
        </div>
        {message && <span className="text-xs text-[var(--color-fg-subtle)]" role="status">{message}</span>}
      </div></Tooltip>
    );
  }

  return (
    <Tooltip content={hint}><label className={cn("flex flex-col gap-1.5", className)}>
      {label && <span className="text-xs font-medium text-[var(--color-fg-muted)]">{labelContent}</span>}
      {children}
      {message && <span className="text-xs text-[var(--color-fg-subtle)]" role="status">{message}</span>}
    </label></Tooltip>
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
