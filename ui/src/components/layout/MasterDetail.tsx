import { cn } from "@/lib/cn";

/**
 * 专业模式页面骨架：中间一栏是本页的条目（管线节点或分类），右侧检查器展示
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
  value?: React.ReactNode;
  /** 管线节点的状态：on 为蓝点，off 为空心圆。 */
  state?: "on" | "off";
  icon?: React.ReactNode;
}

/**
 * 条目列表。chain 为真时是一条处理管线：竖线串起各节点，节点前是状态圆点；
 * 否则是普通分类列表，条目前是图标。
 */
export function NodeList<K extends string>({
  items,
  active,
  onSelect,
  chain = false,
  ariaLabel,
}: {
  items: NodeItem<K>[];
  active: K;
  onSelect: (key: K) => void;
  chain?: boolean;
  ariaLabel: string;
}) {
  return (
    <div role="list" aria-label={ariaLabel} className="relative flex flex-col gap-1.5">
      {chain && <span aria-hidden className="absolute top-[30px] bottom-[30px] left-5 w-px bg-[var(--color-line-strong)]" />}
      {items.map((item) => {
        const selected = item.key === active;
        return (
          <button
            key={item.key}
            type="button"
            role="listitem"
            aria-current={selected ? "true" : undefined}
            onClick={() => onSelect(item.key)}
            className={cn(
              "relative flex w-full flex-col justify-center gap-0.5 rounded-[var(--radius-md)] border pr-3.5 pl-10 text-left transition-colors duration-[var(--dur-fast)]",
              "focus:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent-ring)]",
              item.value === undefined ? "h-11" : "h-[60px]",
              selected
                ? "border-[var(--color-line)] bg-[var(--color-surface)] shadow-[var(--shadow-raise)]"
                : "border-transparent hover:bg-[var(--color-surface-hover)]/60",
            )}
          >
            {chain ? (
              <span
                aria-hidden
                className={cn(
                  "absolute top-1/2 left-[17px] h-[7px] w-[7px] -translate-y-1/2 rounded-full",
                  item.state === "off"
                    ? "border-[1.5px] border-[var(--color-fg-faint)] bg-[var(--color-bg)]"
                    : selected || item.state === "on"
                      ? "bg-[var(--color-accent)]"
                      : "bg-[var(--color-fg-faint)]",
                )}
              />
            ) : (
              item.icon && (
                <span aria-hidden className={cn("absolute top-1/2 left-4 h-4 w-4 -translate-y-1/2 [&>svg]:h-full [&>svg]:w-full", selected ? "text-[var(--color-fg-muted)]" : "text-[var(--color-fg-subtle)]")}>
                  {item.icon}
                </span>
              )
            )}
            {item.value === undefined ? (
              <span className={cn("truncate text-sm", selected ? "text-[var(--color-fg)]" : "text-[var(--color-fg-muted)]")}>{item.label}</span>
            ) : (
              <>
                <span className={cn("text-xs", selected ? "text-[var(--color-fg-muted)]" : "text-[var(--color-fg-subtle)]")}>{item.label}</span>
                <span className="truncate text-sm text-[var(--color-fg)]">{item.value}</span>
              </>
            )}
          </button>
        );
      })}
    </div>
  );
}
