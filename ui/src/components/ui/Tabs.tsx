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
        "inline-flex w-fit flex-wrap items-center gap-1 rounded-xl border border-white/10 bg-white/[0.035] p-1",
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
              "rounded-lg px-4 py-2 text-sm transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-[color-mix(in_srgb,var(--color-accent)_55%,transparent)]",
              isActive
                ? "bg-[var(--color-accent)] font-medium text-[var(--color-accent-contrast)]"
                : "text-white/55 hover:bg-[color-mix(in_srgb,var(--color-accent)_10%,transparent)] hover:text-white/85",
            )}
          >
            {tab.label}
          </button>
        );
      })}
    </div>
  );
}
