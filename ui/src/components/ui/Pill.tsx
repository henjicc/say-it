import { cn } from "@/lib/cn";

/** 玻璃胶囊标签/状态条。 */
export function Pill({
  className,
  children,
  ...props
}: React.HTMLAttributes<HTMLSpanElement>) {
  return (
    <span
      className={cn(
        "inline-flex items-center gap-2 rounded-full border border-white/10 bg-white/5 px-3 py-1 text-xs text-white/70 backdrop-blur-sm",
        className,
      )}
      {...props}
    >
      {children}
    </span>
  );
}
