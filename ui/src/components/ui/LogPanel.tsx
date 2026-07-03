import { forwardRef } from "react";
import { cn } from "@/lib/cn";

/** 等宽日志面板（<pre>），用于事件流/调试输出。 */
export const LogPanel = forwardRef<HTMLPreElement, React.HTMLAttributes<HTMLPreElement>>(
  ({ className, children, ...props }, ref) => (
    <pre
      ref={ref}
      className={cn(
        "max-h-60 overflow-auto rounded-[var(--radius-lg)] border border-[var(--color-line)] bg-[var(--color-bg)] p-3 text-xs leading-relaxed whitespace-pre-wrap text-[var(--color-fg-muted)]",
        "font-mono",
        className,
      )}
      {...props}
    >
      {children}
    </pre>
  ),
);
LogPanel.displayName = "LogPanel";
