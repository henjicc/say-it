import { useUiStore } from "@/store/useUiStore";
import { cn } from "@/lib/cn";
import { useDeveloperStore } from "@/store/useDeveloperStore";
import { WindowDragStrip } from "@/components/layout/WindowDragStrip";
import { PRO_BOTTOM_NAV, proNavGroups, type NavItem } from "./navConfig";

function RailButton({ item, active, onClick }: { item: NavItem; active: boolean; onClick: () => void }) {
  return (
    <button
      type="button"
      aria-current={active ? "page" : undefined}
      onClick={onClick}
      className={cn(
        "flex h-[52px] w-[54px] flex-col items-center justify-center gap-1 rounded-xl text-xs transition-all duration-[var(--dur-fast)]",
        "focus:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent-ring)]",
        active
          ? "bg-[var(--accent-soft)] font-medium text-[var(--color-accent-text)]"
          : "text-[var(--color-fg-subtle)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-fg)]",
      )}
    >
      <span className="h-[18px] w-[18px]">{item.icon}</span>
      {item.label}
    </button>
  );
}

/** 专业模式左侧图标导航。 */
export function Sidebar() {
  const view = useUiStore((s) => s.view);
  const setView = useUiStore((s) => s.setView);
  const developer = useDeveloperStore((s) => s.enabled);

  return (
    <nav aria-label="主导航" className="flex w-[var(--rail-w)] flex-none flex-col items-center gap-1.5 bg-[var(--color-bg-sidebar)] pt-2 pb-4">
      <WindowDragStrip />
      {proNavGroups(developer).map((group, index) => (
        <div key={index} className="flex flex-col items-center gap-1">
          {index > 0 && <span aria-hidden className="my-1.5 h-px w-6 bg-[var(--color-line)]" />}
          {group.map((item) => (
            <RailButton key={item.view} item={item} active={view === item.view} onClick={() => setView(item.view)} />
          ))}
        </div>
      ))}
      <div className="flex-1" />
      {PRO_BOTTOM_NAV.map((item) => (
        <RailButton key={item.view} item={item} active={view === item.view} onClick={() => setView(item.view)} />
      ))}
    </nav>
  );
}
