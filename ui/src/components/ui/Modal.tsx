import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { cn } from "@/lib/cn";
import { Button } from "./Button";

/** 可复用模态框：默认覆盖视口，也可限制在父容器内。 */
export function Modal({
  open,
  onClose,
  title,
  children,
  className,
  overlayClassName,
  scope = "viewport",
  showHeader,
  showCloseButton = true,
  ariaLabel,
}: {
  open: boolean;
  onClose: () => void;
  title?: React.ReactNode;
  children: React.ReactNode;
  className?: string;
  overlayClassName?: string;
  scope?: "viewport" | "container";
  showHeader?: boolean;
  showCloseButton?: boolean;
  ariaLabel?: string;
}) {
  const [rendered, setRendered] = useState(open);
  const visualState = {
    title,
    children,
    className,
    overlayClassName,
    scope,
    showHeader,
    showCloseButton,
    ariaLabel,
  };
  const lastOpenVisualState = useRef(visualState);

  // 调用方通常会在关闭时同时清空消息或选中项。退出动画期间继续使用最后一次
  // 打开状态的完整内容，避免正文先消失导致弹窗尺寸跳变。
  useLayoutEffect(() => {
    if (open) lastOpenVisualState.current = visualState;
  }, [ariaLabel, children, className, open, overlayClassName, scope, showCloseButton, showHeader, title]);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, onClose]);

  useEffect(() => {
    if (open) {
      setRendered(true);
      return;
    }
    const timer = window.setTimeout(() => setRendered(false), 180);
    return () => window.clearTimeout(timer);
  }, [open]);

  if (!rendered) return null;

  const visuals = open ? visualState : lastOpenVisualState.current;
  const shouldShowHeader = visuals.showHeader ?? Boolean(visuals.title);

  return (
    <div
      className={cn(
        "inset-0 z-[var(--z-modal)] grid place-items-center bg-black/70 p-6 backdrop-blur-[3px]",
        open ? "animate-[modal-overlay-in_160ms_var(--ease-out)_both]" : "animate-[modal-overlay-out_160ms_ease-in_both]",
        visuals.scope === "viewport" ? "fixed" : "absolute",
        visuals.overlayClassName,
      )}
      onClick={onClose}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label={visuals.ariaLabel}
        className={cn(
          "flex max-h-[85vh] w-full max-w-3xl flex-col overflow-hidden rounded-[var(--radius-xl)] border border-[var(--color-line-strong)] bg-[var(--color-overlay)] shadow-[var(--shadow-popover)]",
          open ? "animate-[modal-panel-in_180ms_var(--ease-out)_both]" : "animate-[modal-panel-out_140ms_ease-in_both]",
          visuals.className,
        )}
        onClick={(e) => e.stopPropagation()}
      >
        {shouldShowHeader && (
          <div className="flex items-center justify-between border-b border-[var(--color-line)] px-5 py-4">
            <h3 className="text-base font-semibold text-[var(--color-fg)]">{visuals.title}</h3>
            {visuals.showCloseButton && (
              <Button size="sm" onClick={onClose}>
                关闭
              </Button>
            )}
          </div>
        )}
        <div className="min-h-0 flex-1 overflow-y-auto">{visuals.children}</div>
      </div>
    </div>
  );
}
