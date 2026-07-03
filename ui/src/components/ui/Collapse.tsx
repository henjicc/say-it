import { useState, type ReactNode } from "react";
import { cn } from "@/lib/cn";

function ChevronIcon({ open }: { open: boolean }) {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.8}
      strokeLinecap="round"
      strokeLinejoin="round"
      className={cn("h-4 w-4 shrink-0 transition-transform duration-200", open && "rotate-180")}
      aria-hidden
    >
      <path d="m6 9 6 6 6-6" />
    </svg>
  );
}

/** 可折叠区块：点击标题展开/收起内容，用于按供应商/分组组织设置项。 */
export function Collapse({
  title,
  subtitle,
  defaultOpen = false,
  headerRight,
  className,
  headerClassName,
  bodyClassName,
  children,
}: {
  title: ReactNode;
  subtitle?: ReactNode;
  defaultOpen?: boolean;
  headerRight?: ReactNode;
  className?: string;
  headerClassName?: string;
  bodyClassName?: string;
  children: ReactNode;
}) {
  const [open, setOpen] = useState(defaultOpen);

  return (
    <div className={cn("rounded-[var(--radius-lg)] border border-[var(--color-line)] bg-[var(--color-surface)]", className)}>
      <button
        type="button"
        onClick={() => setOpen((value) => !value)}
        className={cn(
          "flex w-full items-center justify-between gap-3 px-4 py-3 text-left",
          headerClassName,
        )}
      >
        <span className="flex min-w-0 flex-1 items-center gap-3">
          <span className="truncate text-sm font-medium text-[var(--color-fg)]">{title}</span>
          {subtitle && <span className="truncate text-xs text-[var(--color-fg-subtle)]">{subtitle}</span>}
        </span>
        <span className="flex items-center gap-2 text-[var(--color-fg-subtle)]">
          {headerRight}
          <ChevronIcon open={open} />
        </span>
      </button>
      <div
        className={cn(
          "grid transition-[grid-template-rows] duration-300 ease-in-out",
          open ? "grid-rows-[1fr]" : "grid-rows-[0fr]",
        )}
      >
        <div className="overflow-hidden">
          <div className={cn("border-t border-[var(--color-line)] px-4 py-4", bodyClassName)}>{children}</div>
        </div>
      </div>
    </div>
  );
}
