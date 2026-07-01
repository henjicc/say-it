import { useUiStore } from "@/store/useUiStore";
import { useProviderStore } from "@/store/useProviderStore";
import { cn } from "@/lib/cn";
import { StatusDot } from "@/components/ui/StatusDot";
import { NAV_ITEMS } from "./navConfig";
import appIcon from "../../../app-icon.png";

export function Sidebar() {
  const view = useUiStore((s) => s.view);
  const setView = useUiStore((s) => s.setView);
  const provider = useProviderStore((s) => s.profiles.find((item) => item.id === "funasr"));
  const configured = !!provider?.status?.hasApiKey;

  return (
    <aside className="flex w-56 flex-none flex-col gap-2 border-r border-white/5 bg-white/[0.02] p-3 backdrop-blur-md">
      <div className="flex items-center gap-2.5 px-2 py-3">
        <img src={appIcon} alt="说吧！" className="h-9 w-9 rounded-xl" />
        <div className="flex flex-col leading-tight">
          <strong className="text-sm font-semibold text-white">说吧！</strong>
          <span className="text-[11px] text-white/40">Say it !</span>
        </div>
      </div>

      <nav className="flex flex-1 flex-col gap-1">
        {NAV_ITEMS.map((item) => {
          const active = view === item.view;
          return (
            <button
              key={item.view}
              type="button"
              onClick={() => setView(item.view)}
              className={cn(
                "group flex items-center gap-3 rounded-xl px-3 py-2.5 text-sm transition-colors",
                active
                  ? "bg-[color-mix(in_srgb,var(--color-accent)_18%,transparent)] font-medium text-white ring-1 ring-[color-mix(in_srgb,var(--color-accent)_24%,transparent)]"
                  : "text-white/55 hover:bg-[color-mix(in_srgb,var(--color-accent)_10%,transparent)] hover:text-white/90",
              )}
            >
              <span className={cn("grid h-5 w-5 place-items-center", active && "text-[var(--color-accent-light)]")}>
                {item.icon}
              </span>
              <span>{item.label}</span>
            </button>
          );
        })}
      </nav>

      <div className="flex items-center gap-2 rounded-xl px-3 py-2.5 text-xs text-white/50">
        <StatusDot tone={configured ? "ok" : "idle"} />
        <span>{configured ? "Fun-ASR 已配置" : "等待配置密钥"}</span>
      </div>
    </aside>
  );
}
