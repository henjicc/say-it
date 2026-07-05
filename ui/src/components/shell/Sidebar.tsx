import { useUiStore } from "@/store/useUiStore";
import { cn } from "@/lib/cn";
import { MAIN_NAV_ITEMS, SECONDARY_NAV_ITEMS, type NavItem, type SecondaryNavItem } from "./navConfig";
import appIcon from "../../../../src-tauri/icons/icon.png";

function NavButton({
  item,
  active,
  subtle = false,
  onClick,
}: {
  item: NavItem | SecondaryNavItem;
  active: boolean;
  subtle?: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        "group flex items-center gap-3 rounded-[var(--radius-md)] transition-colors duration-[var(--dur-fast)]",
        "focus:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent-ring)]",
        subtle ? "px-2.5 py-2 text-xs" : "px-3 py-3 text-sm",
        active
          ? "bg-[var(--color-accent)] text-[var(--color-accent-contrast)]"
          : "text-[var(--color-fg-muted)] hover:bg-[var(--accent-soft)] hover:text-[var(--color-fg)]",
      )}
    >
      <span className={cn("grid flex-none place-items-center", subtle ? "h-4 w-4" : "h-5 w-5")}>
        {item.icon}
      </span>
      <span>{item.label}</span>
    </button>
  );
}

export function Sidebar() {
  const view = useUiStore((s) => s.view);
  const setView = useUiStore((s) => s.setView);
  const aboutOpen = useUiStore((s) => s.aboutOpen);
  const openAbout = useUiStore((s) => s.openAbout);

  return (
    <aside className="flex w-[var(--sidebar-w)] flex-none flex-col gap-2 border-r border-[var(--color-line)] bg-[var(--color-bg-sidebar)] p-3">
      <div className="flex items-center gap-3 px-2 py-4">
        <img src={appIcon} alt="说吧！" className="h-10 w-10 rounded-[var(--radius-lg)]" />
        <div className="flex flex-col leading-tight">
          <strong className="text-[15px] font-semibold text-[var(--color-fg)]">说吧！</strong>
          <span className="text-[11px] text-[var(--color-fg-subtle)]">Say it !</span>
        </div>
      </div>

      <nav className="flex flex-1 flex-col gap-1">
        {MAIN_NAV_ITEMS.map((item) => (
          <NavButton
            key={item.view}
            item={item}
            active={view === item.view}
            onClick={() => setView(item.view)}
          />
        ))}
      </nav>

      <div className="border-t border-[var(--color-line)] pt-2">
        {SECONDARY_NAV_ITEMS.map((item) => (
          <NavButton
            key={item.id}
            item={item}
            active={aboutOpen}
            subtle
            onClick={openAbout}
          />
        ))}
      </div>
    </aside>
  );
}
