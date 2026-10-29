import { cn } from "@/lib/cn";
import { SectionHeader } from "./SectionHeader";
import { HelpLabel } from "./Tooltip";

/**
 * 设置分组：可选的小号标题 + 一块抬起的分组面，组内各行之间自动加分隔线。
 * 组内只放 Row，或与 Row 同样左右分布的整行内容。
 */
export function Group({
  title,
  description,
  right,
  className,
  children,
}: {
  title?: React.ReactNode;
  description?: React.ReactNode;
  right?: React.ReactNode;
  className?: string;
  children: React.ReactNode;
}) {
  return (
    <section className={cn("flex flex-col gap-2", className)}>
      {title && <SectionHeader title={title} description={description} right={right} />}
      <div className="rounded-[var(--radius-lg)] border border-[var(--color-line)] bg-[var(--color-surface)] shadow-[var(--shadow-raise)] [&>*+*]:border-t [&>*+*]:border-[var(--color-line)]">
        {children}
      </div>
    </section>
  );
}

/** 分组里的一行：左侧名称，右侧控件。说明文字只走悬停提示。 */
export function Row({
  label,
  hint,
  controlId,
  message,
  disabled,
  className,
  children,
}: {
  label: React.ReactNode;
  hint?: React.ReactNode;
  controlId?: string;
  /** 必须立即可见的错误或状态，显示在本行下方。 */
  message?: React.ReactNode;
  disabled?: boolean;
  className?: string;
  children?: React.ReactNode;
}) {
  const text = <HelpLabel content={hint}>{label}</HelpLabel>;
  return (
    <div className={cn("flex flex-col justify-center px-3.5", disabled && "opacity-50", className)}>
      <div className="flex min-h-[var(--row-h)] items-center justify-between gap-4">
        {controlId ? (
          <label htmlFor={controlId} className="min-w-0 text-[13px] text-[var(--color-fg)]">{text}</label>
        ) : (
          <span className="min-w-0 text-[13px] text-[var(--color-fg)]">{text}</span>
        )}
        {children && <div className="flex min-w-0 flex-none items-center justify-end gap-2">{children}</div>}
      </div>
      {message && <p className="pb-3 text-xs text-[var(--color-fg-subtle)]" role="status">{message}</p>}
    </div>
  );
}
