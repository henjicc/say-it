import { useEffect } from "react";
import { cn } from "@/lib/cn";
import { useToastStore } from "@/store/useToastStore";
import { IconButton } from "./IconButton";
import { ClearIcon } from "./icons";

const INFO_DURATION_MS = 3000;

/** 轻提示的唯一出口，在窗口底部居中。普通提示自动消失；错误留到用户关闭或下一条提示出现。 */
export function ToastHost() {
  const toast = useToastStore((state) => state.toast);
  const dismiss = useToastStore((state) => state.dismiss);

  useEffect(() => {
    if (!toast || toast.tone === "err") return;
    const timer = window.setTimeout(() => dismiss(toast.id), INFO_DURATION_MS);
    return () => window.clearTimeout(timer);
  }, [toast, dismiss]);

  return (
    // 外层常驻并承担 live region：读屏只播报已存在的 live region 里新增的内容。
    <div role="status" className="pointer-events-none absolute inset-x-0 bottom-6 z-[var(--z-tooltip)] flex justify-center px-6">
      {toast && (
        <div
          key={toast.id}
          role={toast.tone === "err" ? "alert" : undefined}
          className={cn(
            "ui-toast pointer-events-auto flex max-w-[min(480px,100%)] items-start gap-2 rounded-[var(--radius-md)] border bg-[var(--theme-overlay)] py-2 pl-3.5 text-[13px] leading-relaxed shadow-[var(--shadow-popover)]",
            toast.tone === "err"
              ? "border-[var(--color-err)] pr-1.5 text-[var(--color-err)]"
              : "border-[var(--color-line-strong)] pr-3.5 text-[var(--color-fg)]",
          )}
        >
          <span className="min-w-0 max-h-32 overflow-y-auto break-words">{toast.text}</span>
          {toast.tone === "err" && (
            <IconButton size="sm" label="关闭提示" className="flex-none" onClick={() => dismiss(toast.id)}>
              <ClearIcon />
            </IconButton>
          )}
        </div>
      )}
    </div>
  );
}
