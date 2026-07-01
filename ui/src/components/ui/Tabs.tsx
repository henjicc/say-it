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
              "rounded-lg px-4 py-2 text-sm transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-white/30",
              isActive
                ? "bg-white font-medium text-black shadow-[0_8px_24px_rgba(255,255,255,0.08)]"
                : "text-white/55 hover:bg-white/[0.06] hover:text-white/85",
            )}
          >
            {tab.label}
          </button>
        );
      })}
    </div>
  );
}
