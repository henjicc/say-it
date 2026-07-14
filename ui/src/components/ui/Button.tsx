import { forwardRef } from "react";
import { cn } from "@/lib/cn";

type Variant = "primary" | "ghost" | "danger" | "dangerHover";
type Size = "sm" | "md";

export interface ButtonProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: Variant;
  size?: Size;
}

const variants: Record<Variant, string> = {
  primary:
    "bg-[var(--color-accent)] text-[var(--color-accent-contrast)] font-medium hover:bg-[var(--color-accent-light)] active:bg-[var(--color-accent-dark)] disabled:bg-[var(--color-surface-strong)] disabled:text-[var(--color-fg-faint)]",
  ghost:
    "bg-[var(--color-surface)] text-[var(--color-fg)] border border-[var(--color-line)] hover:border-[var(--accent-ring)] hover:bg-[var(--accent-soft)] active:bg-[var(--accent-soft-strong)] disabled:opacity-40",
  danger:
    "bg-[color-mix(in_srgb,var(--color-rec)_15%,transparent)] text-[#ff8589] border border-[color-mix(in_srgb,var(--color-rec)_32%,transparent)] hover:bg-[color-mix(in_srgb,var(--color-rec)_25%,transparent)] disabled:opacity-40",
  dangerHover:
    "bg-[var(--color-surface)] text-[var(--color-fg)] border border-[var(--color-line)] hover:border-[color-mix(in_srgb,var(--color-rec)_32%,transparent)] hover:bg-[color-mix(in_srgb,var(--color-rec)_15%,transparent)] hover:text-[#ff8589] active:bg-[color-mix(in_srgb,var(--color-rec)_25%,transparent)] disabled:opacity-40",
};

const sizes: Record<Size, string> = {
  sm: "h-8 px-3 text-xs",
  md: "h-10 px-4 text-sm",
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
