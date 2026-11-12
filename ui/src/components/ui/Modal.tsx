import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { cn } from "@/lib/cn";
import { Button } from "./Button";

const FOCUSABLE = 'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]';

/** 已打开的弹窗按打开顺序排列；Esc 只交给最上面那个，它关不掉时也不往下传。 */
const openModals: object[] = [];

function focusableIn(dialog: HTMLElement) {
  return Array.from(dialog.querySelectorAll<HTMLElement>(FOCUSABLE)).filter((element) => element.tabIndex >= 0);
}

/** 可复用模态框：默认覆盖视口，也可限制在父容器内。不传 `onClose` 时只能由内容自己结束，Esc、点遮罩都关不掉，也不显示关闭按钮。 */
export function Modal({
  open,
  onClose,
  title,
  children,
  className,
  bodyClassName,
  overlayClassName,
  scope = "viewport",
  showHeader,
  showCloseButton = true,
  ariaLabel,
}: {
  open: boolean;
  onClose?: () => void;
  title?: React.ReactNode;
  children: React.ReactNode;
  className?: string;
  bodyClassName?: string;
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
    bodyClassName,
    overlayClassName,
    scope,
    showHeader,
    showCloseButton,
    ariaLabel,
  };
  const lastOpenVisualState = useRef(visualState);
  const dialogRef = useRef<HTMLDivElement>(null);
  // 打开前的焦点要在打开那次渲染里记下：内容里的 autoFocus 在提交时就抢走焦点，早于任何 effect。
  const wasOpen = useRef(false);
  const restoreFocus = useRef<HTMLElement | null>(null);
  if (open && !wasOpen.current) restoreFocus.current = document.activeElement as HTMLElement | null;
  wasOpen.current = open;

  // 调用方通常会在关闭时同时清空消息或选中项。退出动画期间继续使用最后一次
  // 打开状态的完整内容，避免正文先消失导致弹窗尺寸跳变。
  useLayoutEffect(() => {
    if (open) lastOpenVisualState.current = visualState;
  }, [ariaLabel, bodyClassName, children, className, open, overlayClassName, scope, showCloseButton, showHeader, title]);

  const stackToken = useRef({});
  useEffect(() => {
    if (!open) return;
    const token = stackToken.current;
    openModals.push(token);
    return () => {
      openModals.splice(openModals.indexOf(token), 1);
    };
  }, [open]);

  useEffect(() => {
    if (!open || !onClose) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && openModals.at(-1) === stackToken.current) onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, onClose]);

  // 打开时把焦点移进对话框（内容里有 autoFocus 的保留它），关闭后还给打开前的元素。
  useEffect(() => {
    const dialog = dialogRef.current;
    if (!open || !rendered || !dialog) return;
    if (!dialog.contains(document.activeElement)) (focusableIn(dialog)[0] ?? dialog).focus();
    const previous = restoreFocus.current;
    return () => {
      if (previous?.isConnected) previous.focus();
    };
  }, [open, rendered]);

  // Tab 只在对话框内循环。
  const onDialogKeyDown = (event: React.KeyboardEvent<HTMLDivElement>) => {
    if (event.key !== "Tab") return;
    const items = focusableIn(event.currentTarget);
    if (items.length === 0) {
      event.preventDefault();
      return;
    }
    const first = items[0];
    const last = items[items.length - 1];
    const active = document.activeElement;
    if (event.shiftKey && (active === first || active === event.currentTarget)) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && active === last) {
      event.preventDefault();
      first.focus();
    }
  };

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
        "inset-0 z-[var(--z-modal)] grid place-items-center bg-[var(--color-scrim)] p-6 backdrop-blur-[3px]",
        open ? "animate-[modal-overlay-in_160ms_var(--ease-out)_both]" : "animate-[modal-overlay-out_160ms_ease-in_both]",
        visuals.scope === "viewport" ? "fixed" : "absolute",
        visuals.overlayClassName,
      )}
      onClick={onClose}
    >
      <div
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        tabIndex={-1}
        aria-label={visuals.ariaLabel}
        className={cn(
          "flex max-h-[85vh] w-full max-w-3xl flex-col overflow-hidden rounded-[var(--radius-xl)] border border-[var(--color-line-strong)] bg-[var(--color-overlay)] shadow-[var(--shadow-popover)]",
          open ? "animate-[modal-panel-in_180ms_var(--ease-out)_both]" : "animate-[modal-panel-out_140ms_ease-in_both]",
          "outline-none",
          visuals.className,
        )}
        onClick={(e) => e.stopPropagation()}
        onKeyDown={onDialogKeyDown}
      >
        {shouldShowHeader && (
          <div className="flex shrink-0 items-center justify-between border-b border-[var(--color-line)] px-5 py-4">
            <h3 className="text-base font-semibold text-[var(--color-fg)]">{visuals.title}</h3>
            {visuals.showCloseButton && onClose && (
              <Button size="sm" onClick={onClose}>
                关闭
              </Button>
            )}
          </div>
        )}
        <div className={cn("min-h-0 flex-1 overflow-y-auto", visuals.bodyClassName)}>{visuals.children}</div>
      </div>
    </div>
  );
}
