import { forwardRef } from "react";
import { cn } from "@/lib/cn";

type Variant = "primary" | "ghost" | "text" | "link" | "danger" | "dangerHover" | "dangerText";
type Size = "sm" | "md";

export interface ButtonProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: Variant;
  size?: Size;
}

const variants: Record<Variant, string> = {
  primary:
    "bg-[var(--color-accent-control)] text-[var(--color-accent-control-contrast)] font-medium hover:bg-[var(--color-accent-control-hover)] active:bg-[var(--color-accent-control-active)] disabled:bg-[var(--color-surface-strong)] disabled:text-[var(--color-fg-faint)]",
  ghost:
    "bg-[var(--color-surface)] text-[var(--color-fg)] border border-[var(--color-line-strong)] shadow-[var(--shadow-raise)] hover:bg-[var(--color-surface-hover)] active:bg-[var(--color-surface-strong)] disabled:opacity-40",
  /** 无底无框的次要操作，例如分步弹窗的「上一步」。 */
  text:
    "bg-transparent text-[var(--color-fg-muted)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-fg)] active:bg-[var(--color-surface-strong)] disabled:opacity-40",
  /** 列表行里的正向操作（下载、获取）：强调色文字，不画框。 */
  link:
    "bg-transparent text-[var(--color-accent-text)] hover:bg-[var(--color-surface-hover)] active:bg-[var(--color-surface-strong)] disabled:text-[var(--color-fg-faint)]",
  danger:
    "bg-[color-mix(in_srgb,var(--color-rec)_15%,transparent)] text-[var(--color-err)] border border-[color-mix(in_srgb,var(--color-rec)_32%,transparent)] hover:bg-[color-mix(in_srgb,var(--color-rec)_25%,transparent)] disabled:opacity-40",
  dangerHover:
    "bg-[var(--color-surface)] text-[var(--color-fg)] border border-[var(--color-line-strong)] hover:border-[color-mix(in_srgb,var(--color-rec)_32%,transparent)] hover:bg-[color-mix(in_srgb,var(--color-rec)_15%,transparent)] hover:text-[var(--color-err)] active:bg-[color-mix(in_srgb,var(--color-rec)_25%,transparent)] disabled:opacity-40",
  /** 列表行里的删除、清除：平时灰色，悬停才变红，不画框。 */
  dangerText:
    "bg-transparent text-[var(--color-fg-subtle)] hover:bg-[color-mix(in_srgb,var(--color-rec)_15%,transparent)] hover:text-[var(--color-err)] active:bg-[color-mix(in_srgb,var(--color-rec)_25%,transparent)] disabled:opacity-40",
};

const sizes: Record<Size, string> = {
  sm: "h-[var(--control-h-sm)] px-2.5 text-xs",
  md: "h-[var(--control-h)] px-3.5 text-[13px]",
};

export const Button = forwardRef<HTMLButtonElement, ButtonProps>(
  ({ className, variant = "ghost", size = "md", type = "button", ...props }, ref) => {
    return (
      <button
        ref={ref}
        type={type}
        className={cn(
          "inline-flex items-center justify-center gap-2 rounded-[var(--radius-md)] transition-colors duration-[var(--dur-fast)]",
          "focus:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent-ring)]",
          "disabled:cursor-not-allowed",
          variants[variant],
          sizes[size],
          className,
        )}
        {...props}
      />
    );
  },
);
Button.displayName = "Button";
