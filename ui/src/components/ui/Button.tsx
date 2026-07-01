import { forwardRef } from "react";
import { cn } from "@/lib/cn";

type Variant = "primary" | "ghost" | "danger";
type Size = "sm" | "md";

export interface ButtonProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: Variant;
  size?: Size;
}

const variants: Record<Variant, string> = {
  primary:
    "bg-[var(--color-accent)] text-[var(--color-accent-contrast)] font-medium hover:bg-[var(--color-accent-light)] disabled:bg-white/40 disabled:text-black/50",
  ghost:
    "bg-white/5 text-white border border-white/10 backdrop-blur-[2px] hover:border-[color-mix(in_srgb,var(--color-accent)_42%,transparent)] hover:bg-[color-mix(in_srgb,var(--color-accent)_13%,transparent)] disabled:opacity-40",
  danger:
    "bg-[#ff4d4f]/15 text-[#ff8589] border border-[#ff4d4f]/30 hover:bg-[#ff4d4f]/25 disabled:opacity-40",
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
          "inline-flex items-center justify-center gap-2 rounded-full transition-colors",
          "focus:outline-none focus-visible:ring-2 focus-visible:ring-[color-mix(in_srgb,var(--color-accent)_55%,transparent)]",
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
