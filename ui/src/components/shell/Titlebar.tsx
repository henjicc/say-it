import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { cn } from "@/lib/cn";
import appIcon from "../../../app-icon.png";

const appWindow = getCurrentWindow();

export function Titlebar() {
  const [maximized, setMaximized] = useState(false);

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

  return (
    <div
      data-tauri-drag-region
      className="relative z-[100] flex h-9 flex-none items-center justify-between border-b border-white/5 bg-black/40 pl-3 backdrop-blur-md select-none"
    >
      <div data-tauri-drag-region className="pointer-events-none flex items-center gap-2">
        <img data-tauri-drag-region src={appIcon} alt="" className="h-[18px] w-[18px] rounded-[5px]" />
        <span data-tauri-drag-region className="text-xs font-medium text-white/60">
          说吧！
        </span>
      </div>
      <div className="flex h-full items-stretch">
        <TitleBtn label="最小化" onClick={() => appWindow.minimize()}>
          <path d="M2 6h8" />
        </TitleBtn>
        <TitleBtn label="最大化" onClick={() => appWindow.toggleMaximize()}>
          {maximized ? (
            <>
              <path d="M3.5 3.5V2.5h6v6H8.5" />
              <rect x="2.5" y="3.5" width="6" height="6" rx="1" />
            </>
          ) : (
            <rect x="2.5" y="2.5" width="7" height="7" rx="1" />
          )}
        </TitleBtn>
        <TitleBtn label="关闭" close onClick={() => appWindow.close()}>
          <path d="M3 3l6 6M9 3l-6 6" />
        </TitleBtn>
      </div>
    </div>
  );
}

function TitleBtn({
  label,
  onClick,
  close,
  children,
}: {
  label: string;
  onClick: () => void;
  close?: boolean;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      onClick={onClick}
      className={cn(
        "no-drag grid w-[46px] place-items-center text-white/50 transition-colors",
        close ? "hover:bg-[#e1394b] hover:text-white" : "hover:bg-white/10 hover:text-white",
      )}
    >
      <svg
        viewBox="0 0 12 12"
        className="h-3 w-3"
        fill="none"
        stroke="currentColor"
        strokeWidth={1.2}
        strokeLinecap="round"
        strokeLinejoin="round"
        aria-hidden
      >
        {children}
      </svg>
    </button>
  );
}
