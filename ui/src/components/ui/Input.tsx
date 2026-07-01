import { Children, forwardRef, isValidElement, useEffect, useId, useMemo, useRef, useState } from "react";
import { cn } from "@/lib/cn";

const fieldBase =
  "w-full rounded-xl border border-white/10 bg-white/5 px-4 py-2.5 text-sm text-white " +
  "placeholder:text-white/30 backdrop-blur-[1px] transition-colors " +
  "focus:outline-none focus:border-[color-mix(in_srgb,var(--color-accent)_58%,transparent)] disabled:opacity-50";

export const Input = forwardRef<HTMLInputElement, React.InputHTMLAttributes<HTMLInputElement>>(
  ({ className, ...props }, ref) => (
    <input ref={ref} className={cn(fieldBase, className)} {...props} />
  ),
);
Input.displayName = "Input";

export const Textarea = forwardRef<
  HTMLTextAreaElement,
  React.TextareaHTMLAttributes<HTMLTextAreaElement>
>(({ className, ...props }, ref) => (
  <textarea ref={ref} className={cn(fieldBase, "resize-y leading-relaxed", className)} {...props} />
));
Textarea.displayName = "Textarea";

type SelectChangeEvent = { target: { value: string } };

interface SelectOption {
  value: string;
  label: string;
  disabled?: boolean;
}

export interface SelectProps
  extends Omit<
    React.ButtonHTMLAttributes<HTMLButtonElement>,
    "children" | "onChange" | "value" | "defaultValue"
  > {
  value?: string;
  defaultValue?: string;
  onChange?: (event: SelectChangeEvent) => void;
  children: React.ReactNode;
}

function ChevronDownIcon({ open }: { open: boolean }) {
  return (
    <svg
      viewBox="0 0 20 20"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.8}
      strokeLinecap="round"
      strokeLinejoin="round"
      className={cn("h-4 w-4 text-white/55 transition-transform duration-200", open && "rotate-180 text-white/80")}
      aria-hidden
    >
      <path d="m5 7.5 5 5 5-5" />
    </svg>
  );
}

export function Select({
  className,
  children,
  value,
  defaultValue,
  onChange,
  disabled,
  name,
  id,
  ...props
}: SelectProps) {
  const generatedId = useId();
  const buttonId = id || generatedId;
  const listboxId = `${buttonId}-listbox`;
  const rootRef = useRef<HTMLDivElement>(null);
  const [open, setOpen] = useState(false);
  const [internalValue, setInternalValue] = useState(defaultValue || "");
  const selectedValue = value ?? internalValue;

  const options = useMemo<SelectOption[]>(() => {
    return Children.toArray(children)
      .filter(isValidElement)
      .map((child) => {
        const props = child.props as React.OptionHTMLAttributes<HTMLOptionElement>;
        const optionValue = props.value === undefined ? String(props.children ?? "") : String(props.value);
        return {
          value: optionValue,
          label: String(props.children ?? optionValue),
          disabled: props.disabled,
        };
      });
  }, [children]);

  const selectedOption = options.find((option) => option.value === selectedValue) || options[0];
  const enabledOptions = options.filter((option) => !option.disabled);

  useEffect(() => {
    if (selectedValue || !options[0]) return;
    setInternalValue(options[0].value);
  }, [options, selectedValue]);

  useEffect(() => {
    if (!open) return;

    const handlePointerDown = (event: PointerEvent) => {
      if (!rootRef.current?.contains(event.target as Node)) setOpen(false);
    };

    document.addEventListener("pointerdown", handlePointerDown);
    return () => document.removeEventListener("pointerdown", handlePointerDown);
  }, [open]);

  const commitValue = (nextValue: string, close = true) => {
    if (disabled) return;
    if (value === undefined) setInternalValue(nextValue);
    onChange?.({ target: { value: nextValue } });
    if (close) setOpen(false);
  };

  const moveSelection = (direction: 1 | -1) => {
    if (enabledOptions.length === 0) return;
    const currentIndex = enabledOptions.findIndex((option) => option.value === selectedOption?.value);
    const nextIndex = currentIndex < 0 ? 0 : (currentIndex + direction + enabledOptions.length) % enabledOptions.length;
    commitValue(enabledOptions[nextIndex].value, false);
  };

  const handleKeyDown = (event: React.KeyboardEvent<HTMLButtonElement>) => {
    if (disabled) return;

    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      if (!open) setOpen(true);
      else moveSelection(event.key === "ArrowDown" ? 1 : -1);
      return;
    }

    if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      setOpen((current) => !current);
      return;
    }

    if (event.key === "Escape") {
      setOpen(false);
    }
  };

  return (
    <div ref={rootRef} className={cn("relative w-full", className)}>
      {name && <input type="hidden" name={name} value={selectedOption?.value || ""} />}
      <button
        id={buttonId}
        type="button"
        className={cn(
          fieldBase,
          "flex items-center justify-between gap-3 pr-3 text-left",
          "bg-[linear-gradient(180deg,rgba(255,255,255,0.07),rgba(255,255,255,0.035))]",
          "hover:border-white/18 hover:bg-white/[0.075]",
          open && "border-white/30 bg-white/[0.085]",
          disabled && "cursor-not-allowed opacity-50",
        )}
        disabled={disabled}
        role="combobox"
        aria-controls={listboxId}
        aria-expanded={open}
        aria-haspopup="listbox"
        aria-activedescendant={selectedOption ? `${listboxId}-${selectedOption.value}` : undefined}
        onClick={() => setOpen((current) => !current)}
        onKeyDown={handleKeyDown}
        {...props}
      >
        <span className="min-w-0 truncate">{selectedOption?.label || "请选择"}</span>
        <ChevronDownIcon open={open} />
      </button>

      {open && (
        <div
          id={listboxId}
          role="listbox"
          aria-labelledby={buttonId}
          className="absolute left-0 right-0 top-[calc(100%+6px)] z-50 max-h-64 overflow-auto rounded-xl border border-white/12 bg-[#101010]/95 p-1.5 shadow-[0_18px_48px_rgba(0,0,0,0.55)] backdrop-blur-xl"
        >
          {options.map((option) => {
            const selected = option.value === selectedOption?.value;
            return (
              <button
                key={option.value}
                id={`${listboxId}-${option.value}`}
                type="button"
                role="option"
                aria-selected={selected}
                disabled={option.disabled}
                onClick={() => commitValue(option.value)}
                className={cn(
                  "flex min-h-9 w-full items-center justify-between gap-3 rounded-lg px-3 py-2 text-left text-sm transition-colors",
                  selected
                    ? "bg-[var(--color-accent)] text-[var(--color-accent-contrast)]"
                    : "text-white/72 hover:bg-[color-mix(in_srgb,var(--color-accent)_12%,transparent)] hover:text-white",
                  option.disabled && "cursor-not-allowed opacity-40",
                )}
              >
                <span className="min-w-0 truncate">{option.label}</span>
                {selected && <span className="h-1.5 w-1.5 rounded-full bg-current" aria-hidden />}
              </button>
            );
          })}
        </div>
      )}
    </div>
  );
}
