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
        "relative inline-flex h-6 w-11 flex-none items-center rounded-full transition-colors duration-[var(--dur-fast)]",
        "focus:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent-ring)]",
        checked ? "bg-[var(--color-accent)]" : "bg-[var(--color-surface-strong)]",
        disabled && "cursor-not-allowed opacity-50",
        className,
      )}
    >
      <span
        className={cn(
          "inline-block h-4 w-4 transform rounded-full bg-white shadow-[var(--shadow-sm)] transition-transform duration-[var(--dur-fast)]",
          checked ? "translate-x-6" : "translate-x-1",
        )}
        aria-hidden
      />
    </button>
  );
}
