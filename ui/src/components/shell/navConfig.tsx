import type { ViewKey } from "@/store/useUiStore";
import { BookMarked, Box, Captions, FileAudio, History, Mic, SlidersHorizontal, Sparkles } from "lucide-react";

export interface NavItem {
  view: ViewKey;
  label: string;
  icon: React.ReactNode;
}

const iconProps = { className: "h-full w-full", strokeWidth: 1.6, "aria-hidden": true };

/** 专业模式侧栏：按「功能 | 资源 | 记录」分组，设置单独放在底部。 */
export const PRO_NAV_GROUPS: NavItem[][] = [
  [
    { view: "dictation", label: "听写", icon: <Mic {...iconProps} /> },
    { view: "assistant", label: "助手", icon: <Sparkles {...iconProps} /> },
    { view: "subtitles", label: "字幕", icon: <Captions {...iconProps} /> },
    { view: "transcription", label: "转写", icon: <FileAudio {...iconProps} /> },
  ],
  [
    { view: "models", label: "模型", icon: <Box {...iconProps} /> },
    { view: "lexicon", label: "词库", icon: <BookMarked {...iconProps} /> },
    { view: "history", label: "历史", icon: <History {...iconProps} /> },
  ],
];

export const SETTINGS_NAV: NavItem = { view: "settings", label: "设置", icon: <SlidersHorizontal {...iconProps} /> };

/** 简单模式标题栏里的功能切换。 */
export const SIMPLE_NAV: NavItem[] = [
  { view: "dictation", label: "听写", icon: null },
  { view: "subtitles", label: "字幕", icon: null },
  { view: "transcription", label: "转写", icon: null },
  { view: "history", label: "历史", icon: null },
];

export const SIMPLE_VIEWS: ReadonlySet<ViewKey> = new Set([...SIMPLE_NAV.map((item) => item.view), "settings"]);
