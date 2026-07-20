import type { ViewKey } from "@/store/useUiStore";
import { BookMarked, Captions, ClosedCaption, Info, Mic, Settings } from "lucide-react";

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

const iconClassName = "h-full w-full";
const iconProps = { className: iconClassName, strokeWidth: 1.8, "aria-hidden": true };

export const MAIN_NAV_ITEMS: NavItem[] = [
  {
    view: "dictation",
    label: "语音输入",
    icon: <Mic {...iconProps} />,
  },
  {
    view: "subtitles",
    label: "实时字幕",
    icon: <ClosedCaption {...iconProps} />,
  },
  {
    view: "transcription",
    label: "字幕转写",
    icon: <Captions {...iconProps} />,
  },
  {
    view: "customization",
    label: "热词上下文",
    icon: <BookMarked {...iconProps} />,
  },
  {
    view: "settings",
    label: "设置",
    icon: <Settings {...iconProps} />,
  },
];

export const SECONDARY_NAV_ITEMS: SecondaryNavItem[] = [
  {
    id: "about",
    label: "关于",
    icon: <Info {...iconProps} />,
  },
];

export const VIEW_TITLES: Record<ViewKey, string> = Object.fromEntries(
  MAIN_NAV_ITEMS.map((i) => [i.view, i.label]),
) as Record<ViewKey, string>;
