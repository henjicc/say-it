import { Children, forwardRef, isValidElement, useEffect, useId, useMemo, useRef, useState } from "react";
import { ChevronDown } from "lucide-react";
import { cn } from "@/lib/cn";

const fieldBase =
  "w-full min-h-[var(--control-h)] rounded-[var(--radius-md)] border border-[var(--color-line)] bg-[var(--color-surface)] px-4 py-2.5 text-sm text-[var(--color-fg)] " +
  "placeholder:text-[var(--color-fg-faint)] transition-colors duration-[var(--dur-fast)] " +
  "focus:outline-none focus:border-[var(--accent-ring)] disabled:opacity-50";

export const Input = forwardRef<HTMLInputElement, React.InputHTMLAttributes<HTMLInputElement>>(
  ({ className, ...props }, ref) => (
    <input ref={ref} className={cn(fieldBase, className)} {...props} />
  ),
);
Input.displayName = "Input";

export const NativeSelect = forwardRef<HTMLSelectElement, React.SelectHTMLAttributes<HTMLSelectElement>>(
  ({ className, ...props }, ref) => (
    <select ref={ref} className={cn(fieldBase, className)} {...props} />
  ),
);
NativeSelect.displayName = "NativeSelect";

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
  return <ChevronDown className={cn("h-4 w-4 text-[var(--color-fg-subtle)] transition-transform duration-200", open && "rotate-180 text-[var(--color-fg-muted)]")} strokeWidth={1.8} aria-hidden />;
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
  // 弹层挂载状态：打开时立即挂载，关闭时保留到离开动画结束再卸载
  const [rendered, setRendered] = useState(false);
  // 打开前测一次按钮离视口上下边缘的距离：下方放不下且上方空间更大时，弹层改往上展开
  const [openUpward, setOpenUpward] = useState(false);
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
    if (open) setRendered(true);
  }, [open]);

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

  // 弹层高度受 max-h-[19.5rem] 限制，这里用一个估算值（含搜索框）判断是否需要往上开，
  // 不用等实际渲染出来再量，避免打开瞬间的位置跳变。
  const openDropdown = () => {
    const rect = buttonRef.current?.getBoundingClientRect();
    if (rect) {
      const estimatedHeight = 312 + (searchable ? 54 : 0);
      const spaceBelow = window.innerHeight - rect.bottom;
      const spaceAbove = rect.top;
      setOpenUpward(spaceBelow < estimatedHeight && spaceAbove > spaceBelow);
    }
    setOpen(true);
  };

  const toggleDropdown = () => {
    if (!open) openDropdown();
    else setOpen(false);
  };

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
      if (!open) openDropdown();
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
        toggleDropdown();
      }
      return;
    }

    if (event.key === " " && !searchable) {
      event.preventDefault();
      toggleDropdown();
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
          "hover:border-[var(--color-line-strong)] hover:bg-[var(--color-surface-hover)]",
          open && "border-[var(--accent-ring)] bg-[var(--color-surface-hover)]",
          disabled && "cursor-not-allowed opacity-50",
        )}
        disabled={disabled}
        role="combobox"
        aria-controls={listboxId}
        aria-expanded={open}
        aria-haspopup="listbox"
        aria-activedescendant={selectedOption ? `${listboxId}-${selectedOption.value}` : undefined}
        onClick={toggleDropdown}
        onKeyDown={handleListKeyDown}
        {...props}
      >
        <span className="min-w-0 truncate">{selectedOption?.label || "请选择"}</span>
        <ChevronDownIcon open={open} />
      </button>

      {rendered && (
        <div
          id={listboxId}
          role="listbox"
          aria-labelledby={buttonId}
          onAnimationEnd={() => {
            if (!open) setRendered(false);
          }}
          style={{ transformOrigin: openUpward ? "bottom" : "top" }}
          className={cn(
            "absolute left-0 right-0 z-[var(--z-popover)] max-h-[19.5rem] overflow-hidden rounded-[var(--radius-lg)] border border-[var(--color-line-strong)] bg-[var(--color-overlay)] shadow-[var(--shadow-popover)]",
            openUpward ? "bottom-[calc(100%+6px)]" : "top-[calc(100%+6px)]",
            open
              ? "animate-[dropdown-in_140ms_var(--ease-out)]"
              : "pointer-events-none animate-[dropdown-out_110ms_var(--ease-out)_forwards]",
          )}
        >
          {searchable && (
            <div className="border-b border-[var(--color-line)] p-1.5">
              <input
                ref={searchInputRef}
                type="text"
                value={query}
                onChange={(event) => setQuery(event.target.value)}
                onKeyDown={handleListKeyDown}
                placeholder={searchPlaceholder}
                className="w-full rounded-[var(--radius-sm)] border border-[var(--color-line)] bg-[var(--color-surface)] px-2.5 py-1.5 text-sm text-[var(--color-fg)] placeholder:text-[var(--color-fg-faint)] focus:outline-none focus:border-[var(--accent-ring)]"
              />
            </div>
          )}
          <div className="max-h-64 overflow-auto p-1.5">
          {visibleOptions.length === 0 && (
            <div className="px-3 py-2 text-sm text-[var(--color-fg-subtle)]">无匹配结果</div>
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
                  "flex min-h-9 w-full items-center justify-between gap-3 rounded-[var(--radius-sm)] px-3 py-2 text-left text-sm transition-colors",
                  selected
                    ? "bg-[var(--color-accent)] text-[var(--color-accent-contrast)]"
                    : "text-[var(--color-fg-muted)] hover:bg-[var(--accent-soft)] hover:text-[var(--color-fg)]",
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
