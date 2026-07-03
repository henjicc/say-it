import { cn } from "@/lib/cn";

/** 玻璃胶囊标签/状态条。 */
export function Pill({
  className,
  children,
  ...props
}: React.HTMLAttributes<HTMLSpanElement>) {
  return (
    <span
      className={cn(
        "inline-flex items-center gap-2 rounded-full border border-[var(--color-line)] bg-[var(--color-surface)] px-3 py-1 text-xs text-[var(--color-fg-muted)]",
        className,
      )}
      {...props}
    >
      {children}
    </span>
  );
}
