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
  /** 打开时在选项上方显示搜索框，按 label/value 过滤，选项较多时（如字体、设备列表）适用。 */
  searchable?: boolean;
  searchPlaceholder?: string;
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
  searchable,
  searchPlaceholder = "搜索…",
  ...props
}: SelectProps) {
  const generatedId = useId();
  const buttonId = id || generatedId;
  const listboxId = `${buttonId}-listbox`;
  const rootRef = useRef<HTMLDivElement>(null);
  const buttonRef = useRef<HTMLButtonElement>(null);
  const searchInputRef = useRef<HTMLInputElement>(null);
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
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

  // 当前值可能来自异步加载的列表（字体/设备），列表还没到位或值已不在列表里时，
  // 直接显示原始值而不是错误地回退成第一项。
  const selectedOption =
    options.find((option) => option.value === selectedValue) ||
    (selectedValue ? { value: selectedValue, label: selectedValue } : options[0]);

  const visibleOptions = useMemo(() => {
    if (!searchable || !query.trim()) return options;
    const q = query.trim().toLowerCase();
    return options.filter((option) => option.label.toLowerCase().includes(q) || option.value.toLowerCase().includes(q));
  }, [options, searchable, query]);
  const enabledOptions = visibleOptions.filter((option) => !option.disabled);

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

  useEffect(() => {
    if (!open) {
      setQuery("");
      return;
    }
    if (searchable) searchInputRef.current?.focus();
  }, [open, searchable]);

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

  const handleListKeyDown = (event: React.KeyboardEvent<HTMLElement>) => {
    if (disabled) return;

    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      if (!open) setOpen(true);
      else moveSelection(event.key === "ArrowDown" ? 1 : -1);
      return;
    }

    if (event.key === "Enter") {
      event.preventDefault();
      if (open && searchable) {
        const activeInList = enabledOptions.find((option) => option.value === selectedOption?.value);
        const target = activeInList ?? enabledOptions[0];
        if (target) commitValue(target.value);
      } else {
        setOpen((current) => !current);
      }
      return;
    }

    if (event.key === " " && !searchable) {
      event.preventDefault();
      setOpen((current) => !current);
      return;
    }

    if (event.key === "Escape") {
      event.preventDefault();
      setOpen(false);
      buttonRef.current?.focus();
    }
  };

  return (
    <div ref={rootRef} className={cn("relative w-full", className)}>
      {name && <input type="hidden" name={name} value={selectedOption?.value || ""} />}
      <button
        ref={buttonRef}
        id={buttonId}
        type="button"
        className={cn(
          fieldBase,
          "flex items-center justify-between gap-3 pr-3 text-left",
          "hover:border-white/18 hover:bg-white/[0.06]",
          open && "border-white/30 bg-white/[0.06]",
          disabled && "cursor-not-allowed opacity-50",
        )}
        disabled={disabled}
        role="combobox"
        aria-controls={listboxId}
        aria-expanded={open}
        aria-haspopup="listbox"
        aria-activedescendant={selectedOption ? `${listboxId}-${selectedOption.value}` : undefined}
        onClick={() => setOpen((current) => !current)}
        onKeyDown={handleListKeyDown}
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
          className="absolute left-0 right-0 top-[calc(100%+6px)] z-50 max-h-[19.5rem] overflow-hidden rounded-xl border border-white/12 bg-[#101010]/95 shadow-[0_18px_48px_rgba(0,0,0,0.55)] backdrop-blur-xl"
        >
          {searchable && (
            <div className="border-b border-white/10 p-1.5">
              <input
                ref={searchInputRef}
                type="text"
                value={query}
                onChange={(event) => setQuery(event.target.value)}
                onKeyDown={handleListKeyDown}
                placeholder={searchPlaceholder}
                className="w-full rounded-lg border border-white/10 bg-white/5 px-2.5 py-1.5 text-sm text-white placeholder:text-white/30 focus:outline-none focus:border-[color-mix(in_srgb,var(--color-accent)_58%,transparent)]"
              />
            </div>
          )}
          <div className="max-h-64 overflow-auto p-1.5">
          {visibleOptions.length === 0 && (
            <div className="px-3 py-2 text-sm text-white/40">无匹配结果</div>
          )}
          {visibleOptions.map((option) => {
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
        </div>
      )}
    </div>
  );
}
