import { useCallback, useEffect, useId, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { ChevronDown, Search } from "lucide-react";
import { Input } from "@/components/ui/Input";
import { cn } from "@/lib/cn";
import {
  asrModelModeLabel,
  type AsrModelMode,
} from "@/features/asr/modelRegistry";

export interface ModelPickerOption {
  value: string;
  label: string;
  triggerLabel?: string;
  providerId?: string;
  providerLabel?: string;
  filterProviderId?: string;
  filterProviderLabel?: string;
  mode?: AsrModelMode;
}

interface PickerLayout {
  left: number;
  top?: number;
  bottom?: number;
  width: number;
  height: number;
  openUpward: boolean;
}

export interface ModelPickerProps {
  value: string;
  options: readonly ModelPickerOption[];
  onChange: (value: string) => void;
  id?: string;
  disabled?: boolean;
  className?: string;
  "aria-label"?: string;
  panelLabel?: string;
  searchPlaceholder?: string;
  placeholder?: string;
}

const modeFilters: Array<{ value: "all" | AsrModelMode; label: string }> = [
  { value: "all", label: "全部" },
  { value: "realtime", label: "实时" },
  { value: "nonRealtime", label: "非实时" },
];

const optionProviderId = (option: ModelPickerOption) => option.filterProviderId || option.providerId || "other";
const optionProviderLabel = (option: ModelPickerOption) => option.filterProviderLabel || option.providerLabel || "其他";

function FilterChip({
  active,
  children,
  ariaLabel,
  onClick,
}: {
  active: boolean;
  children: React.ReactNode;
  ariaLabel: string;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      aria-label={ariaLabel}
      aria-pressed={active}
      onClick={onClick}
      className={cn(
        "shrink-0 rounded-full border px-3 py-1.5 text-xs transition-colors duration-[var(--dur-fast)]",
        active
          ? "border-[var(--accent-ring)] bg-[var(--accent-soft-strong)] text-[var(--color-accent-light)]"
          : "border-[var(--color-line)] bg-[var(--color-surface)] text-[var(--color-fg-muted)] hover:border-[var(--color-line-strong)] hover:text-[var(--color-fg)]",
      )}
    >
      {children}
    </button>
  );
}

export function ModelPicker({
  value,
  options,
  onChange,
  id,
  disabled,
  className,
  "aria-label": ariaLabel,
  panelLabel = "选择模型",
  searchPlaceholder = "搜索模型名称或供应商…",
  placeholder = "请选择模型",
}: ModelPickerProps) {
  const generatedId = useId();
  const buttonId = id || generatedId;
  const panelId = `${buttonId}-model-panel`;
  const listboxId = `${buttonId}-model-listbox`;
  const buttonRef = useRef<HTMLButtonElement>(null);
  const panelRef = useRef<HTMLDivElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const selectedOptionRef = useRef<HTMLButtonElement>(null);
  const [open, setOpen] = useState(false);
  const [rendered, setRendered] = useState(false);
  const [layout, setLayout] = useState<PickerLayout | null>(null);
  const [query, setQuery] = useState("");
  const [providerId, setProviderId] = useState("all");
  const [mode, setMode] = useState<"all" | AsrModelMode>("all");

  const selected = options.find((option) => option.value === value);
  const providers = useMemo(() => {
    const unique = new Map<string, string>();
    options.forEach((option) => unique.set(optionProviderId(option), optionProviderLabel(option)));
    return Array.from(unique, ([value, label]) => ({ value, label }));
  }, [options]);
  const supportsModeFilter = options.some((option) => Boolean(option.mode));
  const visibleOptions = useMemo(() => {
    const normalizedQuery = query.trim().toLocaleLowerCase();
    return options.filter((option) => {
      if (providerId !== "all" && optionProviderId(option) !== providerId) return false;
      if (supportsModeFilter && mode !== "all" && option.mode !== mode) return false;
      if (!normalizedQuery) return true;
      return [option.label, optionProviderLabel(option), option.value]
        .some((text) => text.toLocaleLowerCase().includes(normalizedQuery));
    });
  }, [mode, options, providerId, query, supportsModeFilter]);

  const measurePanel = useCallback(() => {
    const rect = buttonRef.current?.getBoundingClientRect();
    if (!rect) return;
    const viewportMargin = 8;
    const triggerGap = 6;
    const availableWidth = Math.max(280, window.innerWidth - viewportMargin * 2);
    const width = Math.min(520, availableWidth);
    const left = Math.min(
      Math.max(viewportMargin, rect.left),
      Math.max(viewportMargin, window.innerWidth - width - viewportMargin),
    );
    const spaceBelow = Math.max(0, window.innerHeight - rect.bottom - viewportMargin - triggerGap);
    const spaceAbove = Math.max(0, rect.top - viewportMargin - triggerGap);
    const preferredHeight = 440;
    const openUpward = spaceBelow >= preferredHeight
      ? false
      : spaceAbove >= preferredHeight || spaceAbove > spaceBelow;
    const availableHeight = openUpward ? spaceAbove : spaceBelow;
    setLayout({
      left,
      top: openUpward ? undefined : rect.bottom + triggerGap,
      bottom: openUpward ? window.innerHeight - rect.top + triggerGap : undefined,
      width,
      height: Math.max(180, Math.min(preferredHeight, availableHeight)),
      openUpward,
    });
  }, []);

  useEffect(() => {
    if (!open) return;
    setRendered(true);
    measurePanel();
    const frame = window.requestAnimationFrame(() => {
      searchRef.current?.focus();
      selectedOptionRef.current?.scrollIntoView?.({ block: "center" });
    });
    window.addEventListener("resize", measurePanel);
    window.addEventListener("scroll", measurePanel, true);
    return () => {
      window.cancelAnimationFrame(frame);
      window.removeEventListener("resize", measurePanel);
      window.removeEventListener("scroll", measurePanel, true);
    };
  }, [measurePanel, open]);

  useEffect(() => {
    if (open) return;
    setQuery("");
    setProviderId("all");
    setMode("all");
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const handlePointerDown = (event: PointerEvent) => {
      const target = event.target as Node;
      if (!buttonRef.current?.contains(target) && !panelRef.current?.contains(target)) setOpen(false);
    };
    document.addEventListener("pointerdown", handlePointerDown);
    return () => document.removeEventListener("pointerdown", handlePointerDown);
  }, [open]);

  const closeAndFocus = () => {
    setOpen(false);
    buttonRef.current?.focus();
  };
  const commit = (nextValue: string) => {
    onChange(nextValue);
    setOpen(false);
  };

  return (
    <div className={cn("relative w-full", className)}>
      <button
        ref={buttonRef}
        id={buttonId}
        type="button"
        role="combobox"
        aria-label={ariaLabel}
        aria-controls={panelId}
        aria-expanded={open}
        aria-haspopup="dialog"
        disabled={disabled}
        onClick={() => setOpen((current) => !current)}
        onKeyDown={(event) => {
          if (event.key === "ArrowDown" || event.key === "Enter" || event.key === " ") {
            event.preventDefault();
            setOpen(true);
          } else if (event.key === "Escape") closeAndFocus();
        }}
        className={cn(
          "flex h-[var(--control-h)] w-full items-center justify-between gap-3 rounded-[var(--radius-md)] border border-[var(--color-line)] bg-[var(--color-surface)] px-4 py-2.5 text-left text-sm text-[var(--color-fg)] transition-colors duration-[var(--dur-fast)]",
          "hover:border-[var(--color-line-strong)] hover:bg-[var(--color-surface-hover)] focus:outline-none focus:border-[var(--accent-ring)]",
          open && "border-[var(--accent-ring)] bg-[var(--color-surface-hover)]",
          disabled && "cursor-not-allowed opacity-50",
        )}
      >
        <span className="min-w-0 truncate">{selected?.triggerLabel || selected?.label || value || placeholder}</span>
        <ChevronDown
          className={cn("h-4 w-4 shrink-0 text-[var(--color-fg-subtle)] transition-transform", open && "rotate-180")}
          aria-hidden
        />
      </button>

      {rendered && layout && createPortal(
        <div
          ref={panelRef}
          id={panelId}
          role="dialog"
          aria-label={panelLabel}
          onKeyDown={(event) => {
            if (event.key === "Escape") {
              event.preventDefault();
              closeAndFocus();
            }
          }}
          onAnimationEnd={() => {
            if (!open) setRendered(false);
          }}
          style={{
            left: layout.left,
            top: layout.top,
            bottom: layout.bottom,
            width: layout.width,
            height: layout.height,
            transformOrigin: layout.openUpward ? "bottom" : "top",
          }}
          className={cn(
            "fixed z-[var(--z-portal-popover)] flex flex-col overflow-hidden rounded-[var(--radius-lg)] border border-[var(--color-line-strong)] bg-[var(--color-overlay)] shadow-[var(--shadow-popover)]",
            open
              ? "animate-[dropdown-in_140ms_var(--ease-out)]"
              : "pointer-events-none animate-[dropdown-out_110ms_var(--ease-out)_forwards]",
          )}
        >
          <div className="shrink-0 border-b border-[var(--color-line)] p-3">
            <div className="relative">
              <Search className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-[var(--color-fg-faint)]" aria-hidden />
              <Input
                ref={searchRef}
                value={query}
                aria-label={`搜索${panelLabel.replace(/^选择/u, "")}`}
                placeholder={searchPlaceholder}
                onChange={(event) => setQuery(event.target.value)}
                className="pl-9"
              />
            </div>

            <div className="mt-3 grid gap-2">
              <div className="flex min-w-0 items-center gap-2">
                <span className="w-12 shrink-0 text-xs text-[var(--color-fg-subtle)]">供应商</span>
                <div className="flex min-w-0 gap-1.5 overflow-x-auto pb-1" role="group" aria-label="按供应商筛选">
                  <FilterChip active={providerId === "all"} ariaLabel="供应商：全部" onClick={() => setProviderId("all")}>全部</FilterChip>
                  {providers.map((provider) => (
                    <FilterChip
                      key={provider.value}
                      active={providerId === provider.value}
                      ariaLabel={`供应商：${provider.label}`}
                      onClick={() => setProviderId(provider.value)}
                    >
                      {provider.label}
                    </FilterChip>
                  ))}
                </div>
              </div>
              {supportsModeFilter && <div className="flex items-center gap-2">
                <span className="w-12 shrink-0 text-xs text-[var(--color-fg-subtle)]">识别方式</span>
                <div className="flex gap-1.5" role="group" aria-label="按识别方式筛选">
                  {modeFilters.map((filter) => (
                    <FilterChip
                      key={filter.value}
                      active={mode === filter.value}
                      ariaLabel={`识别方式：${filter.label}`}
                      onClick={() => setMode(filter.value)}
                    >
                      {filter.label}
                    </FilterChip>
                  ))}
                </div>
              </div>}
            </div>
          </div>

          <div id={listboxId} role="listbox" aria-label={panelLabel.replace(/^选择/u, "")} className="min-h-0 flex-1 overflow-y-auto py-2 pl-2 pr-1">
            {visibleOptions.length === 0 && (
              <div className="px-3 py-8 text-center text-sm text-[var(--color-fg-subtle)]">没有符合条件的模型</div>
            )}
            {visibleOptions.map((option) => {
              const isSelected = option.value === value;
              return (
                <button
                  key={option.value}
                  ref={isSelected ? selectedOptionRef : undefined}
                  type="button"
                  role="option"
                  aria-label={option.label}
                  aria-selected={isSelected}
                  onClick={() => commit(option.value)}
                  className={cn(
                    "flex w-full items-center gap-3 rounded-[var(--radius-md)] py-2.5 pl-3 pr-2 text-left transition-colors duration-[var(--dur-fast)]",
                    isSelected
                      ? "bg-[var(--accent-soft-strong)] text-[var(--color-fg)]"
                      : "text-[var(--color-fg-muted)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-fg)]",
                  )}
                >
                  <span className="min-w-0 flex-1">
                    <span className="block whitespace-normal break-words text-sm font-medium leading-5">{option.label}</span>
                    <span className="mt-0.5 block whitespace-normal break-words text-xs text-[var(--color-fg-subtle)]">{optionProviderLabel(option)}</span>
                  </span>
                  {option.mode && <span className="shrink-0 rounded-full border border-[var(--color-line)] px-2 py-1 text-[11px] text-[var(--color-fg-subtle)]">
                    {asrModelModeLabel(option.mode)}
                  </span>}
                </button>
              );
            })}
          </div>
        </div>,
        document.body,
      )}
    </div>
  );
}
