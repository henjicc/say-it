/** 标题栏里的品牌标记：图形取自 src-tauri/icons/source/icon-small.svg，底板跟随用户强调色。 */
export function AppMark() {
  return (
    <span
      role="img"
      aria-label="说吧！"
      className="block h-4 w-4 flex-none overflow-hidden rounded-[4px] bg-[var(--color-accent-control)] text-[var(--color-accent-control-contrast)]"
    >
      <svg width="16" height="16" viewBox="0 0 48 48" fill="currentColor" aria-hidden>
        <g opacity="0.82">
          <rect x="6" y="18" width="6" height="12" rx="3" />
          <rect x="15" y="12" width="6" height="24" rx="3" />
        </g>
        <path d="M24 6H42V9H36V39H42V42H24V39H30V9H24Z" />
      </svg>
    </span>
  );
}
