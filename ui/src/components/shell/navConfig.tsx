import type { ViewKey } from "@/store/useUiStore";

export interface NavItem {
  view: ViewKey;
  label: string;
  icon: React.ReactNode;
}

export interface SecondaryNavItem {
  id: "about";
  label: string;
  icon: React.ReactNode;
}

const iconProps = {
  viewBox: "0 0 24 24",
  fill: "none",
  stroke: "currentColor",
  strokeWidth: 1.8,
  strokeLinecap: "round" as const,
  strokeLinejoin: "round" as const,
  "aria-hidden": true,
};

export const MAIN_NAV_ITEMS: NavItem[] = [
  {
    view: "dictation",
    label: "语音输入",
    icon: (
      <svg {...iconProps}>
        <path d="M12 14a3 3 0 0 0 3-3V6a3 3 0 0 0-6 0v5a3 3 0 0 0 3 3Z" />
        <path d="M19 11a7 7 0 0 1-14 0M12 18v3" />
      </svg>
    ),
  },
  {
    view: "subtitles",
    label: "实时字幕",
    icon: (
      <svg {...iconProps}>
        <rect x="4" y="5" width="16" height="14" rx="3" />
        <path d="M8 10h8M8 14h5" />
      </svg>
    ),
  },
  {
    view: "transcription",
    label: "字幕转写",
    icon: (
      <svg {...iconProps}>
        <path d="M7 3.5h6l4 4V20a1.5 1.5 0 0 1-1.5 1.5h-7A1.5 1.5 0 0 1 7 20v-3.5" />
        <path d="M13 3.5V8h4" />
        <path d="M3.5 12h2l1.2-3.2 2.2 6.4 1.6-4.2H13" />
      </svg>
    ),
  },
  {
    view: "settings",
    label: "设置",
    icon: (
      <svg {...iconProps}>
        <circle cx="12" cy="12" r="3" />
        <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09a1.65 1.65 0 0 0-1-1.51 1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09a1.65 1.65 0 0 0 1.51-1 1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1Z" />
      </svg>
    ),
  },
];

export const SECONDARY_NAV_ITEMS: SecondaryNavItem[] = [
  {
    id: "about",
    label: "关于",
    icon: (
      <svg {...iconProps}>
        <circle cx="12" cy="12" r="8" />
        <path d="M12 10.25h.01M11.25 13h1.5v3h-1.5" />
      </svg>
    ),
  },
];

export const VIEW_TITLES: Record<ViewKey, string> = Object.fromEntries(
  MAIN_NAV_ITEMS.map((i) => [i.view, i.label]),
) as Record<ViewKey, string>;
