import { Fragment } from "react";
import { cn } from "@/lib/cn";

/**
 * 专业模式页面骨架：中间一栏是本页的分类列表，右侧检查器展示
 * 选中条目的设置。两栏各自滚动。
 */
export function MasterDetail({
  title,
  titleRight,
  list,
  listFooter,
  children,
}: {
  title: React.ReactNode;
  titleRight?: React.ReactNode;
  list: React.ReactNode;
  listFooter?: React.ReactNode;
  children: React.ReactNode;
}) {
  return (
    <div className="flex min-h-0 flex-1">
      <section className="flex w-[var(--list-col-w)] flex-none flex-col border-r border-[var(--color-line)] bg-[var(--color-bg)] px-4 pt-5 pb-4">
        <div className="flex h-8 items-center justify-between gap-2 pr-1 pl-1.5">
          <h1 className="text-lg font-semibold text-[var(--color-fg)]">{title}</h1>
          {titleRight}
        </div>
        <div className="mt-4 min-h-0 flex-1 overflow-y-auto">{list}</div>
        {listFooter && <div className="mt-3 flex flex-none gap-2 [&>*]:flex-1">{listFooter}</div>}
      </section>
      <main className="min-w-0 flex-1 overflow-y-auto bg-[var(--color-panel)] px-8 pt-5 pb-8">{children}</main>
    </div>
  );
}

/** 检查器标题行：标题 + 右侧开关或操作。 */
export function InspectorHeader({ title, right, className }: { title: React.ReactNode; right?: React.ReactNode; className?: string }) {
  return (
    <div className={cn("mb-4 flex h-8 items-center justify-between gap-3", className)}>
      <h2 className="text-lg font-semibold text-[var(--color-fg)]">{title}</h2>
      {right && <div className="flex items-center gap-2">{right}</div>}
    </div>
  );
}

/** 检查器内容：分组之间统一 16px 间距。 */
export function InspectorBody({ children, className }: { children: React.ReactNode; className?: string }) {
  return <div className={cn("flex flex-col gap-4", className)}>{children}</div>;
}

export interface NodeItem<K extends string> {
  key: K;
  label: string;
  icon: React.ReactNode;
  /** 可整体关闭的条目处于关闭状态时，在行尾标出。 */
  off?: boolean;
  /** 与上一条之间画分隔线，区分不同性质的条目。 */
  dividerBefore?: boolean;
  /** 本条之前的小标题，标出一组同类条目；可带一个操作按钮。 */
  headingBefore?: { label: string; action?: React.ReactNode };
  /** 行尾状态点：已配置为实心点，尚未配置为空心圈。 */
  status?: { tone: "ok" | "unset"; label: string };
}

const STATUS_DOT: Record<NonNullable<NodeItem<string>["status"]>["tone"], string> = {
  ok: "bg-[var(--color-ok)]",
  unset: "border border-[var(--color-fg-faint)]",
};

/** 中栏分类列表：单行、图标 + 名称。条目顺序即使用顺序。 */
export function NodeList<K extends string>({
  items,
  active,
  onSelect,
  ariaLabel,
}: {
  items: NodeItem<K>[];
  active: K;
  onSelect: (key: K) => void;
  ariaLabel: string;
}) {
  return (
    <div role="list" aria-label={ariaLabel} className="flex flex-col gap-1.5">
      {items.map((item) => {
        const selected = item.key === active;
        return (
          <Fragment key={item.key}>
            {item.dividerBefore && <span aria-hidden className="mx-3 my-1 h-px bg-[var(--color-line)]" />}
            {item.headingBefore && (
              <div role="listitem" className="mt-3 flex h-7 items-center justify-between pr-1 pl-4 text-xs text-[var(--color-fg-faint)]">
                <span>{item.headingBefore.label}</span>
                {item.headingBefore.action}
              </div>
            )}
            <button
              type="button"
              role="listitem"
              aria-current={selected ? "true" : undefined}
              onClick={() => onSelect(item.key)}
              className={cn(
                "flex h-11 w-full items-center gap-3 rounded-[var(--radius-md)] border px-4 text-left transition-colors duration-[var(--dur-fast)]",
                "focus:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent-ring)]",
                selected
                  ? "border-transparent bg-[var(--color-accent-control)] text-[var(--color-accent-control-contrast)]"
                  : "border-transparent hover:bg-[var(--color-surface-hover)]/60",
              )}
            >
              <span aria-hidden className={cn("h-4 w-4 flex-none [&>svg]:h-full [&>svg]:w-full", !selected && "text-[var(--color-fg-subtle)]")}>
                {item.icon}
              </span>
              <span className={cn("min-w-0 flex-1 truncate text-sm", !selected && "text-[var(--color-fg-muted)]")}>{item.label}</span>
              {item.off && <span className={cn("flex-none text-xs", !selected && "text-[var(--color-fg-subtle)]")}>已关闭</span>}
              {item.status && (
                <span
                  role="img"
                  aria-label={item.status.label}
                  className={cn("h-2 w-2 flex-none rounded-full", STATUS_DOT[item.status.tone], selected && item.status.tone === "ok" && "bg-[var(--color-accent-control-contrast)]")}
                />
              )}
            </button>
          </Fragment>
        );
      })}
    </div>
  );
}
