import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Copy, Minus, Square, X } from "lucide-react";
import { cn } from "@/lib/cn";
import { isMacOS } from "@/lib/platform";
import { useUiStore } from "@/store/useUiStore";
import { SETTINGS_NAV, SIMPLE_NAV } from "./navConfig";
import { AppMark } from "./AppMark";

const appWindow = getCurrentWindow();

/**
 * 标题栏。专业模式只有品牌与窗口按钮；简单模式把功能切换放进标题栏中部，
 * 设置作为图标放在窗口按钮左侧。macOS 左侧留给系统红绿灯。
 */
export function Titlebar() {
  const mode = useUiStore((s) => s.layoutMode);
  const view = useUiStore((s) => s.view);
  const setView = useUiStore((s) => s.setView);
  const [maximized, setMaximized] = useState(false);
  // 关闭/最小化后窗口离开视野,WebView 收不到 mouseleave,:hover 会残留;
  // 点击时先压制 hover 样式,等鼠标真正在标题栏移动时再恢复。
  const [hoverMuted, setHoverMuted] = useState(false);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    const sync = async () => {
      try {
        setMaximized(await appWindow.isMaximized());
      } catch {
        /* noop */
      }
    };
    sync();
    appWindow.onResized(sync).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  const simple = mode === "simple";

  return (
    <div
      data-tauri-drag-region
      className={cn(
        "relative z-[var(--z-titlebar)] flex h-[var(--titlebar-h)] flex-none items-center bg-[var(--color-bg-titlebar)] select-none",
        isMacOS ? "pl-[78px]" : "pl-3.5",
      )}
    >
      {!(isMacOS && simple) && (
        <div data-tauri-drag-region className="flex items-center gap-2 pr-1.5">
          <AppMark />
          {!simple && <span data-tauri-drag-region className="text-[13px] font-semibold text-[var(--color-fg)]">说吧！</span>}
        </div>
      )}

      {simple ? (
        <nav aria-label="功能" data-tauri-drag-region className="no-drag flex flex-1 justify-center gap-0.5">
          {SIMPLE_NAV.map((item) => (
            <button
              key={item.view}
              type="button"
              aria-current={view === item.view ? "page" : undefined}
              onClick={() => setView(item.view)}
              className={cn(
                "h-7 rounded-[7px] px-3 text-[13px] transition-colors duration-[var(--dur-fast)]",
                "focus:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent-ring)]",
                view === item.view ? "bg-[var(--color-surface)] text-[var(--color-fg)]" : "text-[var(--color-fg-subtle)] hover:text-[var(--color-fg)]",
              )}
            >
              {item.label}
            </button>
          ))}
        </nav>
      ) : (
        <div data-tauri-drag-region className="flex-1 self-stretch" />
      )}

      {simple && (
        <button
          type="button"
          aria-label={SETTINGS_NAV.label}
          aria-current={view === "settings" ? "page" : undefined}
          onClick={() => setView("settings")}
          className={cn(
            "no-drag mr-1 grid h-7 w-8 place-items-center rounded-[7px] transition-colors duration-[var(--dur-fast)]",
            "focus:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent-ring)]",
            view === "settings" ? "bg-[var(--color-surface)] text-[var(--color-fg)]" : "text-[var(--color-fg-subtle)] hover:text-[var(--color-fg)]",
          )}
        >
          <span className="h-[15px] w-[15px]">{SETTINGS_NAV.icon}</span>
        </button>
      )}

      {!isMacOS && <div
        className="flex h-full items-stretch"
        onPointerMove={hoverMuted ? () => setHoverMuted(false) : undefined}
      >
        <TitleBtn
          label="最小化"
          hoverMuted={hoverMuted}
          onClick={() => {
            setHoverMuted(true);
            appWindow.minimize();
          }}
        >
          <Minus className="h-3 w-3" strokeWidth={1.4} aria-hidden />
        </TitleBtn>
        <TitleBtn
          label={maximized ? "还原" : "最大化"}
          hoverMuted={hoverMuted}
          onClick={() => appWindow.toggleMaximize()}
        >
          {maximized ? <Copy className="h-3 w-3" strokeWidth={1.25} aria-hidden /> : <Square className="h-3 w-3" strokeWidth={1.25} aria-hidden />}
        </TitleBtn>
        <TitleBtn
          label="关闭"
          close
          hoverMuted={hoverMuted}
          onClick={() => {
            setHoverMuted(true);
            appWindow.close();
          }}
        >
          <X className="h-3 w-3" strokeWidth={1.4} aria-hidden />
        </TitleBtn>
      </div>}
    </div>
  );
}

function TitleBtn({
  label,
  onClick,
  close,
  hoverMuted,
  children,
}: {
  label: string;
  onClick: () => void;
  close?: boolean;
  hoverMuted?: boolean;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      onClick={onClick}
      className={cn(
        "no-drag grid h-full w-[46px] place-items-center text-[var(--color-fg-muted)] transition-colors duration-[var(--dur-fast)]",
        !hoverMuted &&
          (close
            ? "hover:bg-[#e1394b] hover:text-white"
            : "hover:bg-[var(--color-surface)] hover:text-[var(--color-fg)]"),
      )}
    >
      {children}
    </button>
  );
}
