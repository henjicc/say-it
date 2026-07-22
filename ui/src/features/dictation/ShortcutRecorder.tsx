import { useEffect, useRef, useState } from "react";
import { Button } from "@/components/ui/Button";
import { Input } from "@/components/ui/Input";
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

  const toggleCapture = () => {
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
    <div className="flex items-stretch gap-2">
      <div className="relative min-w-0 flex-1">
        <Input
          readOnly
          disabled={disabled}
          aria-label={ariaLabel}
          value={capturing ? "请按下按键…" : label}
          placeholder="未设置"
          className={cn(capturing && "border-[var(--accent-ring)]", !capturing && label && onClear && "pr-11")}
        />
        {!capturing && label && onClear && !disabled && (
          <InputAffixButton label="清除快捷键" onClick={() => void onClear()}>
            <ClearIcon />
          </InputAffixButton>
        )}
      </div>
      <Button
        disabled={disabled}
        className="shrink-0 self-stretch"
        aria-label={capturing ? `取消录入${ariaLabel}` : `录入${ariaLabel}`}
        onClick={toggleCapture}
      >
        {capturing ? "取消" : "录入"}
      </Button>
    </div>
  );
}
