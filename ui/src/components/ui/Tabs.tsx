import { useId, useRef } from "react";
import { cn } from "@/lib/cn";

export interface TabItem<K extends string = string> {
  key: K;
  label: string;
}

interface TabsProps<K extends string> {
  id?: string;
  ariaLabel?: string;
  tabs: TabItem<K>[];
  active: K;
  onChange: (key: K) => void;
  className?: string;
}

export function Tabs<K extends string>({ id, ariaLabel = "页面选项", tabs, active, onChange, className }: TabsProps<K>) {
  const generatedId = useId().replace(/:/g, "");
  const tabsId = id ?? `tabs-${generatedId}`;
  const buttonRefs = useRef<Array<HTMLButtonElement | null>>([]);

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
      className={cn(
        "inline-flex h-[var(--control-h)] w-fit items-center gap-1 rounded-[var(--radius-lg)] border border-[var(--color-line)] bg-[var(--color-surface)] p-1",
        className,
      )}
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
              "h-[var(--control-h-sm)] rounded-[var(--radius-md)] px-4 text-sm transition-colors duration-[var(--dur-fast)] focus:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent-ring)]",
              isActive
                ? "bg-[var(--color-accent)] text-[var(--color-accent-contrast)]"
                : "text-[var(--color-fg-subtle)] hover:bg-[var(--accent-soft)] hover:text-[var(--color-fg-muted)]",
            )}
          >
            {tab.label}
          </button>
        );
      })}
    </div>
  );
}
