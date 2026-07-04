import { cn } from "@/lib/cn";

interface CheckboxProps
  extends Omit<React.InputHTMLAttributes<HTMLInputElement>, "type" | "size"> {
  /** md=18px（默认），sm=14px（紧凑列表内使用）。 */
  size?: "sm" | "md";
}

/**
 * 自定义复选框：隐藏原生控件，用与设计令牌一致的方框 + 勾选动画呈现。
 * 原生 input 以透明层覆盖在方框上，保留点击、聚焦与 label 关联等原生行为。
 */
export function Checkbox({ className, size = "md", ...props }: CheckboxProps) {
  const box = size === "sm" ? "h-3.5 w-3.5" : "h-[18px] w-[18px]";
  return (
    <span className={cn("relative inline-flex flex-none", box, className)}>
      <input
        type="checkbox"
        className="peer absolute inset-0 z-10 m-0 cursor-pointer opacity-0 disabled:cursor-not-allowed"
        {...props}
      />
      <span
        aria-hidden
        className={cn(
          "pointer-events-none flex h-full w-full items-center justify-center rounded-[var(--radius-sm)]",
          "border border-[var(--color-line-strong)] bg-[var(--color-surface)] transition-colors duration-[var(--dur-fast)]",
          "peer-hover:border-[var(--color-fg-subtle)]",
          "peer-checked:border-transparent peer-checked:bg-[var(--color-accent)]",
          "peer-focus-visible:ring-2 peer-focus-visible:ring-[var(--accent-ring)]",
          "peer-disabled:opacity-50",
          "[&>svg]:scale-50 [&>svg]:opacity-0 [&>svg]:transition-[opacity,transform] [&>svg]:duration-[var(--dur-fast)]",
          "peer-checked:[&>svg]:scale-100 peer-checked:[&>svg]:opacity-100",
        )}
      >
        <svg
          viewBox="0 0 16 16"
          fill="none"
          stroke="currentColor"
          strokeWidth={2.4}
          strokeLinecap="round"
          strokeLinejoin="round"
          className="h-[72%] w-[72%] text-[var(--color-accent-contrast)]"
        >
          <path d="m3.5 8.5 3 3 6-7" />
        </svg>
      </span>
    </span>
  );
}
