import { cn } from "@/lib/cn";

/** 开关：受控切换控件，选中态填充强调色。 */
export function Switch({
  checked,
  onChange,
  disabled,
  label,
  className,
  id,
}: {
  checked: boolean;
  onChange: (checked: boolean) => void;
  disabled?: boolean;
  label?: string;
  className?: string;
  id?: string;
}) {
  return (
    <button
      type="button"
      role="switch"
      id={id}
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={cn(
        "relative inline-flex h-5 w-9 flex-none items-center rounded-full transition-colors duration-[var(--dur-fast)]",
        "focus:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent-ring)]",
        checked ? "bg-[var(--color-accent-control)]" : "bg-[var(--color-line-strong)]",
        disabled && "cursor-not-allowed opacity-50",
        className,
      )}
    >
      <span
        className={cn(
          "inline-block h-3.5 w-3.5 transform rounded-full transition-transform duration-[var(--dur-fast)]",
          checked ? "translate-x-[19px] bg-white" : "translate-x-[3px] bg-[var(--color-fg-muted)]",
        )}
        aria-hidden
      />
    </button>
  );
}
