import { cn } from "@/lib/cn";

/**
 * 输入框内嵌的图标按钮（复制、显隐密码、清除快捷键等）。
 *
 * 这个形态一度在 SecretInput、ObsOverlayPanel、DictationShortcutsPanel、SubtitleGeneralPanel
 * 里各写了一遍，尺寸 h-7/h-8、圆角 sm/md、hover 底色和 disabled 透明度都各不相同。
 * 视觉上是同一个控件，就只留这一份实现；调用方只传行为，不再自带定位与配色。
 *
 * 默认绝对定位在输入框右侧，因此外层容器需要 `relative`，且输入框要留出右内边距（`pr-12`）。
 */
export function InputAffixButton({
  label,
  title,
  onClick,
  disabled,
  pressed,
  keepFocus = false,
  className,
  children,
}: {
  label: string;
  title?: string;
  onClick: () => void;
  disabled?: boolean;
  /** 切换类按钮（如显隐密码）传入当前状态，暴露给辅助技术。 */
  pressed?: boolean;
  /** 点击时不让输入框失焦——密钥显隐这类操作需要保持光标位置。 */
  keepFocus?: boolean;
  className?: string;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      aria-label={label}
      title={title ?? label}
      aria-pressed={pressed}
      onMouseDown={keepFocus ? (event) => event.preventDefault() : undefined}
      onClick={onClick}
      disabled={disabled}
      className={cn(
        "absolute right-2 top-1/2 grid h-8 w-8 -translate-y-1/2 place-items-center",
        "rounded-[var(--radius-md)] text-[var(--color-fg-subtle)] transition-colors duration-[var(--dur-fast)]",
        "hover:bg-[var(--color-surface-strong)] hover:text-[var(--color-fg)]",
        "focus:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent-ring)]",
        "disabled:cursor-not-allowed disabled:opacity-40",
        className,
      )}
    >
      {children}
    </button>
  );
}
