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
  /**
   * 可选：按需读取已保存的明文，用于让用户核对当前配置。
   *
   * 不提供时（多数场景）已保存的值不可见，只能覆写。提供后，草稿为空时点「显示」
   * 会调用它，返回的明文只用于当次展示，不会进入 `onDraftChange` 或保存请求，
   * 并在失焦、再次隐藏或开始输入时立即清除。读取失败请在回调内部自行提示并返回
   * 空串——本组件只负责不展示。
   */
  onRevealStored?: () => Promise<string>;
}

/**
 * 持久化密钥输入框。掩码仅作为 placeholder 展示，永远不会进入 input value 或保存回调。
 */
export function SecretInput({
  draftValue,
  hasStoredValue,
  onDraftChange,
  onRevealStored,
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
  // 已保存的明文只在用户主动点「显示」之后短暂存在于此。
  const [revealedStored, setRevealedStored] = useState("");

  useEffect(() => {
    if (draftValue || !hasStoredValue) return;
    setVisible(false);
  }, [draftValue, hasStoredValue]);

  const showingStoredMask = hasStoredValue && !editing && !visible && !draftValue;
  const canToggle = Boolean(draftValue) || Boolean(onRevealStored && hasStoredValue);
  const shownValue = !draftValue && visible ? revealedStored : draftValue;

  const hideSecret = () => {
    setVisible(false);
    setRevealedStored("");
    setEditing(inputRef.current === document.activeElement);
  };

  const toggleVisibility = () => {
    if (visible) {
      hideSecret();
      return;
    }
    if (draftValue) {
      setVisible(true);
      return;
    }
    if (!onRevealStored || !hasStoredValue) return;
    void onRevealStored().then((secret) => {
      if (!secret) return;
      setRevealedStored(secret);
      setEditing(false);
      setVisible(true);
    });
  };

  return (
    <div className="relative">
      <Input
        {...props}
        ref={inputRef}
        type={visible ? "text" : "password"}
        value={shownValue}
        placeholder={showingStoredMask ? STORED_SECRET_MASK : placeholder}
        disabled={disabled}
        onFocus={(event) => {
          setEditing(true);
          onFocus?.(event);
        }}
        onBlur={(event) => {
          setEditing(false);
          setVisible(false);
          setRevealedStored("");
          onBlur?.(event);
        }}
        onChange={(event) => {
          setEditing(true);
          setRevealedStored("");
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
