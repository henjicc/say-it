import { useUiStore } from "@/store/useUiStore";
import { cn } from "@/lib/cn";
import { PRO_NAV_GROUPS, SETTINGS_NAV, type NavItem } from "./navConfig";

function RailButton({ item, active, onClick }: { item: NavItem; active: boolean; onClick: () => void }) {
  return (
    <button
      type="button"
      aria-current={active ? "page" : undefined}
      onClick={onClick}
      className={cn(
        "flex h-[52px] w-[52px] flex-col items-center justify-center gap-1 rounded-[var(--radius-md)] text-[11px] transition-colors duration-[var(--dur-fast)]",
        "focus:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent-ring)]",
        active
          ? "bg-[var(--color-surface)] text-[var(--color-fg)]"
          : "text-[var(--color-fg-subtle)] hover:text-[var(--color-fg)]",
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

  return (
    <nav aria-label="主导航" className="flex w-[var(--rail-w)] flex-none flex-col items-center gap-1 bg-[var(--color-bg-sidebar)] pt-1 pb-3">
      {PRO_NAV_GROUPS.map((group, index) => (
        <div key={index} className="flex flex-col items-center gap-1">
          {index > 0 && <span aria-hidden className="my-1.5 h-px w-6 bg-[var(--color-line)]" />}
          {group.map((item) => (
            <RailButton key={item.view} item={item} active={view === item.view} onClick={() => setView(item.view)} />
          ))}
        </div>
      ))}
      <div className="flex-1" />
      <RailButton item={SETTINGS_NAV} active={view === "settings"} onClick={() => setView("settings")} />
    </nav>
  );
}
