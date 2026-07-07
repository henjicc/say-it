import { useEffect, useRef } from "react";
import { cn } from "@/lib/cn";

export function CueTextarea({
  value,
  onChange,
  onFocus,
  onBlur,
  textareaRef,
}: {
  value: string;
  onChange: (text: string) => void;
  onFocus?: () => void;
  onBlur?: () => void;
  textareaRef?: (node: HTMLTextAreaElement | null) => void;
}) {
  const localRef = useRef<HTMLTextAreaElement | null>(null);
  useEffect(() => {
    const el = localRef.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${el.scrollHeight}px`;
  }, [value]);

  return (
    <textarea
      ref={(node) => {
        localRef.current = node;
        textareaRef?.(node);
      }}
      value={value}
      rows={1}
      onChange={(event) => onChange(event.target.value)}
      onFocus={onFocus}
      onBlur={onBlur}
      placeholder="（空字幕）"
      className={cn(
        "mt-1.5 w-full resize-none overflow-hidden rounded-[var(--radius-sm)] border border-transparent bg-transparent px-2 py-1 text-sm leading-6",
        "text-[var(--color-fg-muted)] transition-colors duration-[var(--dur-fast)] placeholder:text-[var(--color-fg-faint)]",
        "hover:border-[var(--color-line)] focus:border-[var(--accent-ring)] focus:bg-[var(--color-surface)] focus:outline-none",
      )}
    />
  );
}
