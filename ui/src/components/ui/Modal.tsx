import { useEffect } from "react";
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
  ariaLabel?: string;
}) {
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, onClose]);

  if (!open) return null;

  const shouldShowHeader = showHeader ?? Boolean(title);

  return (
    <div
      className={cn(
        "inset-0 z-[var(--z-modal)] grid place-items-center bg-black/60 p-6",
        scope === "viewport" ? "fixed" : "absolute",
        overlayClassName,
      )}
      onClick={onClose}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label={ariaLabel}
        className={cn(
          "flex max-h-[85vh] w-full max-w-3xl flex-col overflow-hidden rounded-[var(--radius-xl)] border border-[var(--color-line-strong)] bg-[var(--color-overlay)] shadow-[var(--shadow-popover)]",
          className,
        )}
        onClick={(e) => e.stopPropagation()}
      >
        {shouldShowHeader && (
          <div className="flex items-center justify-between border-b border-[var(--color-line)] px-5 py-4">
            <h3 className="text-base font-semibold text-[var(--color-fg)]">{title}</h3>
            <Button size="sm" onClick={onClose}>
              关闭
            </Button>
          </div>
        )}
        <div className="min-h-0 flex-1 overflow-y-auto">{children}</div>
      </div>
    </div>
  );
}
