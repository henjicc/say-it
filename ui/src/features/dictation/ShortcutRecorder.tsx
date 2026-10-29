import { useEffect, useRef, useState } from "react";
import { Button } from "@/components/ui/Button";
import { InputAffixButton } from "@/components/ui/InputAffixButton";
import { ClearIcon } from "@/components/ui/icons";
import { cn } from "@/lib/cn";
import {
  beginShortcutCapture,
  shortcutLabel,
  type ShortcutCombo,
} from "@/features/dictation/hotkeys";

interface ShortcutRecorderProps {
  value: ShortcutCombo;
  onChange: (shortcut: ShortcutCombo) => void | Promise<void>;
  onClear?: () => void | Promise<void>;
  disabled?: boolean;
  ariaLabel?: string;
}

export function ShortcutRecorder({ value, onChange, onClear, disabled, ariaLabel = "快捷键" }: ShortcutRecorderProps) {
  const [capturing, setCapturing] = useState(false);
  const cancelRef = useRef<(() => void) | null>(null);

  useEffect(() => () => cancelRef.current?.(), []);
  useEffect(() => {
    if (disabled) cancelRef.current?.();
  }, [disabled]);
  useEffect(() => {
    const cancel = () => cancelRef.current?.();
    window.addEventListener("blur", cancel);
    return () => window.removeEventListener("blur", cancel);
  }, []);

  const toggleCapture = () => {
    if (disabled) return;
    if (capturing) {
      cancelRef.current?.();
      return;
    }
    setCapturing(true);
    cancelRef.current = beginShortcutCapture(
      async (shortcut) => {
        cancelRef.current = null;
        setCapturing(false);
        await onChange(shortcut);
      },
      () => {
        cancelRef.current = null;
        setCapturing(false);
      },
    );
  };

  const label = shortcutLabel(value);
  return (
    <div
      className="relative min-w-0"
      onBlur={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget)) cancelRef.current?.();
      }}
      onKeyDown={(event) => {
        if (disabled || capturing || !label || !onClear || event.ctrlKey || event.altKey || event.shiftKey || event.metaKey) return;
        if (event.key === "Delete" || event.key === "Backspace") {
          event.preventDefault();
          void onClear();
        }
      }}
    >
      <Button
        disabled={disabled}
        aria-label={`${ariaLabel}：${capturing ? "请按下按键，Esc 取消" : label || "点击设置"}`}
        aria-pressed={capturing}
        onClick={() => { if (!capturing) toggleCapture(); }}
        className={cn(
          "w-full justify-start text-left",
          (label || capturing) && "pr-12",
          capturing && "border-[var(--accent-ring)]",
          !capturing && !label && "text-[var(--color-fg-subtle)]",
        )}
      >
        <span className="truncate" aria-live="polite">
          {capturing ? "请按下按键…" : label || "点击设置"}
        </span>
      </Button>
      {(capturing || label) && (
        <InputAffixButton
          label={capturing ? `取消录制${ariaLabel}` : `重新录制${ariaLabel}`}
          title=""
          disabled={disabled}
          onClick={toggleCapture}
        >
          <ClearIcon />
        </InputAffixButton>
      )}
    </div>
  );
}
