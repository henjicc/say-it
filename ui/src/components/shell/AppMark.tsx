/** 标题栏里的品牌标记：强调色圆角方块 + 声波。 */
export function AppMark() {
  return (
    <span
      role="img"
      aria-label="说吧！"
      className="grid h-4 w-4 flex-none place-items-center rounded-[4px] bg-[var(--color-accent-control)] text-[var(--color-accent-control-contrast)]"
    >
      <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3" strokeLinecap="round" aria-hidden>
        <path d="M4 10v4M9 6v12M14 9v6M19 11v2" />
      </svg>
    </span>
  );
}
