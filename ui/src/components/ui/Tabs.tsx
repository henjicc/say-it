import { cn } from "@/lib/cn";

export interface TabItem<K extends string = string> {
  key: K;
  label: string;
}

interface TabsProps<K extends string> {
  tabs: TabItem<K>[];
  active: K;
  onChange: (key: K) => void;
  className?: string;
}

export function Tabs<K extends string>({ tabs, active, onChange, className }: TabsProps<K>) {
  if (tabs.length <= 1) return null;

  return (
    <div
      className={cn(
        "inline-flex w-fit flex-wrap items-center gap-1 rounded-[var(--radius-lg)] border border-[var(--color-line)] bg-[var(--color-surface)] p-1",
        className,
      )}
    >
      {tabs.map((tab) => {
        const isActive = tab.key === active;
        return (
          <button
            key={tab.key}
            type="button"
            onClick={() => onChange(tab.key)}
            className={cn(
              "rounded-[var(--radius-md)] px-4 py-2 text-sm transition-colors duration-[var(--dur-fast)] focus:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent-ring)]",
              isActive
                ? "bg-[var(--color-accent)] font-medium text-[var(--color-accent-contrast)]"
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
