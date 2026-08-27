import { useEffect, useRef, useState } from "react";
import { Eye, EyeOff } from "lucide-react";
import { Input } from "./Input";
import { cn } from "@/lib/cn";
import { InputAffixButton } from "./InputAffixButton";

const STORED_SECRET_MASK = "•".repeat(32);

export interface SecretInputProps
  extends Omit<React.InputHTMLAttributes<HTMLInputElement>, "type" | "value" | "defaultValue" | "onChange" | "size"> {
  draftValue: string;
  hasStoredValue: boolean;
  onDraftChange: (value: string) => void;
}

/**
 * 持久化密钥输入框。掩码仅作为 placeholder 展示，永远不会进入 input value 或保存回调。
 */
export function SecretInput({
  draftValue,
  hasStoredValue,
  onDraftChange,
  className,
  placeholder,
  disabled,
  onFocus,
  onBlur,
  ...props
}: SecretInputProps) {
  const inputRef = useRef<HTMLInputElement>(null);
  const [editing, setEditing] = useState(false);
  const [visible, setVisible] = useState(false);

  useEffect(() => {
    if (draftValue || !hasStoredValue) return;
    setVisible(false);
  }, [draftValue, hasStoredValue]);

  const showingStoredMask = hasStoredValue && !editing && !visible && !draftValue;
  const canToggle = Boolean(draftValue);

  const hideSecret = () => {
    setVisible(false);
    setEditing(inputRef.current === document.activeElement);
  };

  const toggleVisibility = () => {
    if (visible) {
      hideSecret();
      return;
    }
    if (draftValue) setVisible(true);
  };

  return (
    <div className="relative">
      <Input
        {...props}
        ref={inputRef}
        type={visible ? "text" : "password"}
        value={draftValue}
        placeholder={showingStoredMask ? STORED_SECRET_MASK : placeholder}
        disabled={disabled}
        onFocus={(event) => {
          setEditing(true);
          onFocus?.(event);
        }}
        onBlur={(event) => {
          setEditing(false);
          setVisible(false);
          onBlur?.(event);
        }}
        onChange={(event) => {
          setEditing(true);
          onDraftChange(event.target.value);
        }}
        className={cn(
          "pr-11",
          showingStoredMask && "placeholder:text-[var(--color-fg)]",
          className,
        )}
      />
      <InputAffixButton
        label={visible ? "隐藏密钥" : "显示密钥"}
        pressed={visible}
        keepFocus
        onClick={toggleVisibility}
        disabled={disabled || !canToggle}
      >
        {visible ? (
          <EyeOff className="h-4 w-4" aria-hidden />
        ) : (
          <Eye className="h-4 w-4" aria-hidden />
        )}
      </InputAffixButton>
    </div>
  );
}
