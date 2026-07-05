import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { cn } from "@/lib/cn";

const appWindow = getCurrentWindow();

export function Titlebar() {
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

  return (
    <div
      data-tauri-drag-region
      className="relative z-[var(--z-titlebar)] flex h-[var(--titlebar-h)] flex-none items-center justify-end border-b border-[var(--color-line)] bg-[var(--color-bg-titlebar)] select-none"
    >
      <div
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
          <path d="M2 6h8" />
        </TitleBtn>
        <TitleBtn
          label="最大化"
          hoverMuted={hoverMuted}
          onClick={() => appWindow.toggleMaximize()}
        >
          {maximized ? (
            <>
              <path d="M3.5 3.5V2.5h6v6H8.5" />
              <rect x="2.5" y="3.5" width="6" height="6" rx="1" />
            </>
          ) : (
            <rect x="2.5" y="2.5" width="7" height="7" rx="1" />
          )}
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
        "no-drag grid h-full w-[46px] place-items-center text-[var(--color-fg-subtle)] transition-colors duration-[var(--dur-fast)]",
        !hoverMuted &&
          (close
            ? "hover:bg-[#e1394b] hover:text-white"
            : "hover:bg-[var(--color-surface-strong)] hover:text-[var(--color-fg)]"),
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
