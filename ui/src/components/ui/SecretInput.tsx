import { useEffect, useRef, useState } from "react";
import { Eye, EyeOff, LoaderCircle } from "lucide-react";
import { Input } from "./Input";
import { cn } from "@/lib/cn";
import { InputAffixButton } from "./InputAffixButton";

const STORED_SECRET_MASK = "•".repeat(32);

export interface SecretInputProps
  extends Omit<React.InputHTMLAttributes<HTMLInputElement>, "type" | "value" | "defaultValue" | "onChange" | "size"> {
  draftValue: string;
  hasStoredValue: boolean;
  onDraftChange: (value: string) => void;
  revealStoredValue?: () => Promise<string>;
  onRevealError?: (error: unknown) => void;
}

/**
 * 持久化密钥输入框。掩码仅作为 placeholder 展示，永远不会进入 input value 或保存回调。
 */
export function SecretInput({
  draftValue,
  hasStoredValue,
  onDraftChange,
  revealStoredValue,
  onRevealError,
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
  const [revealedValue, setRevealedValue] = useState("");
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    if (draftValue || !hasStoredValue) return;
    setVisible(false);
    setRevealedValue("");
  }, [draftValue, hasStoredValue]);

  const showingStoredMask = hasStoredValue && !editing && !visible && !draftValue;
  const inputValue = visible && !draftValue ? revealedValue : draftValue;
  const canToggle = Boolean(draftValue || (hasStoredValue && revealStoredValue));

  const hideSecret = () => {
    setVisible(false);
    setRevealedValue("");
    setEditing(inputRef.current === document.activeElement);
  };

  const toggleVisibility = async () => {
    if (visible) {
      hideSecret();
      return;
    }
    if (draftValue) {
      setVisible(true);
      return;
    }
    if (!hasStoredValue || !revealStoredValue) return;

    setLoading(true);
    try {
      const secret = await revealStoredValue();
      setRevealedValue(secret);
      setVisible(true);
      setEditing(false);
    } catch (error) {
      hideSecret();
      onRevealError?.(error);
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="relative">
      <Input
        {...props}
        ref={inputRef}
        type={visible ? "text" : "password"}
        value={inputValue}
        placeholder={showingStoredMask ? STORED_SECRET_MASK : placeholder}
        disabled={disabled}
        aria-busy={loading || undefined}
        onFocus={(event) => {
          setEditing(true);
          onFocus?.(event);
        }}
        onBlur={(event) => {
          setEditing(false);
          setVisible(false);
          setRevealedValue("");
          onBlur?.(event);
        }}
        onChange={(event) => {
          setEditing(true);
          setRevealedValue("");
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
        onClick={() => void toggleVisibility()}
        disabled={disabled || loading || !canToggle}
      >
        {loading ? (
          <LoaderCircle className="h-4 w-4 animate-spin" aria-hidden />
        ) : visible ? (
          <EyeOff className="h-4 w-4" aria-hidden />
        ) : (
          <Eye className="h-4 w-4" aria-hidden />
        )}
      </InputAffixButton>
    </div>
  );
}
