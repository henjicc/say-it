import { useId, useRef } from "react";
import { cn } from "@/lib/cn";

export interface TabItem<K extends string = string> {
  key: K;
  label: string;
}

/**
 * Tabs 的层级变体。默认使用 page，保证现有调用方的视觉与行为不变；
 * 新页面应根据导航层级显式选择 subpage 或 view。
 */
export type TabsVariant = "page" | "subpage" | "view";

interface TabsProps<K extends string> {
  id?: string;
  ariaLabel?: string;
  tabs: TabItem<K>[];
  active: K;
  onChange: (key: K) => void;
  variant?: TabsVariant;
  className?: string;
}

const VARIANT_STYLES: Record<TabsVariant, {
  list: string;
  button: string;
  active: string;
  inactive: string;
}> = {
  page: {
    list: "inline-flex h-[var(--control-h)] w-fit items-center gap-1 rounded-[var(--radius-lg)] border border-[var(--color-line)] bg-[var(--color-surface)] p-1",
    button: "h-[var(--control-h-sm)] rounded-[var(--radius-md)] px-4 text-sm",
    active: "bg-[var(--color-accent)] text-[var(--color-accent-contrast)]",
    inactive: "text-[var(--color-fg-subtle)] hover:bg-[var(--accent-soft)] hover:text-[var(--color-fg-muted)]",
  },
  subpage: {
    list: "flex h-[var(--control-h)] w-full items-end gap-5 border-b border-[var(--color-line)]",
    button: "-mb-px h-[var(--control-h-sm)] border-b-2 border-transparent px-1 text-sm",
    active: "border-[var(--color-accent)] text-[var(--color-fg)]",
    inactive: "text-[var(--color-fg-subtle)] hover:text-[var(--color-fg-muted)]",
  },
  view: {
    list: "inline-flex h-[var(--control-h-sm)] w-fit items-center gap-0.5 rounded-[var(--radius-md)] border border-[var(--color-line)] bg-[var(--color-surface)] p-0.5",
    button: "h-full rounded-[var(--radius-sm)] px-3 text-xs",
    active: "bg-[var(--accent-soft)] text-[var(--color-accent-light)]",
    inactive: "text-[var(--color-fg-subtle)] hover:bg-[var(--accent-soft)] hover:text-[var(--color-fg-muted)]",
  },
};

export function Tabs<K extends string>({
  id,
  ariaLabel = "页面选项",
  tabs,
  active,
  onChange,
  variant = "page",
  className,
}: TabsProps<K>) {
  const generatedId = useId().replace(/:/g, "");
  const tabsId = id ?? `tabs-${generatedId}`;
  const buttonRefs = useRef<Array<HTMLButtonElement | null>>([]);
  const styles = VARIANT_STYLES[variant];

  if (tabs.length <= 1) return null;

  const moveFocus = (nextIndex: number) => {
    const target = tabs[nextIndex];
    if (!target) return;
    onChange(target.key);
    window.requestAnimationFrame(() => buttonRefs.current[nextIndex]?.focus());
  };

  const handleKeyDown = (event: React.KeyboardEvent<HTMLButtonElement>, index: number) => {
    let nextIndex: number | undefined;
    if (event.key === "ArrowRight") nextIndex = (index + 1) % tabs.length;
    if (event.key === "ArrowLeft") nextIndex = (index - 1 + tabs.length) % tabs.length;
    if (event.key === "Home") nextIndex = 0;
    if (event.key === "End") nextIndex = tabs.length - 1;
    if (nextIndex === undefined) return;
    event.preventDefault();
    moveFocus(nextIndex);
  };

  return (
    <div
      id={tabsId}
      role="tablist"
      aria-label={ariaLabel}
      className={cn(styles.list, className)}
    >
      {tabs.map((tab, index) => {
        const isActive = tab.key === active;
        return (
          <button
            ref={(node) => { buttonRefs.current[index] = node; }}
            key={tab.key}
            id={`${tabsId}-${tab.key}-tab`}
            type="button"
            role="tab"
            aria-selected={isActive}
            aria-controls={id ? `${tabsId}-${tab.key}-panel` : undefined}
            tabIndex={isActive ? 0 : -1}
            onClick={() => onChange(tab.key)}
            onKeyDown={(event) => handleKeyDown(event, index)}
            className={cn(
              styles.button,
              "transition-colors duration-[var(--dur-fast)] focus:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent-ring)]",
              isActive
                ? styles.active
                : styles.inactive,
            )}
          >
            {tab.label}
          </button>
        );
      })}
    </div>
  );
}
