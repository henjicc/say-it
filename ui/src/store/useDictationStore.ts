import { create } from "zustand";
import type { DictationShortcutProfile, ShortcutCombo } from "@/features/dictation/hotkeys";

type Tone = "" | "ok" | "err";

export type ActiveAppContextStatus =
  | "captured"
  | "empty"
  | "blocked"
  | "sensitive"
  | "timedOut"
  | "unsupported"
  | "failed";

export interface ActiveAppContextSummary {
  status: ActiveAppContextStatus;
  captureMethod: "nativeText" | "ocr";
  source?: "ia2Text" | "uiaTextPattern" | "win32Message" | "officeNative" | "msaa" | "clipboardDeep" | "ocr" | null;
  appName: string;
  processName: string;
  windowTitle?: string | null;
  preview: string;
  elapsedMs: number;
  truncated: boolean;
}

interface DictationState {
  statusText: string;
  statusTone: Tone;
  latestText: string;
  log: string;
  recording: boolean;
  shortcutLabel: string;
  shortcut: ShortcutCombo;
  shortcutProfiles: DictationShortcutProfile[];
  injectMethod: "paste" | "type";
  pressHoldMode: boolean;
  activeAppContext?: ActiveAppContextSummary;
}

export const useDictationStore = create<DictationState>(() => ({
  statusText: "语音输入未激活",
  statusTone: "",
  latestText: "",
  log: "",
  recording: false,
  shortcutLabel: "",
  shortcut: { keyCode: "", ctrl: false, shift: false, alt: false, meta: false },
  shortcutProfiles: [],
  injectMethod: "paste",
  pressHoldMode: false,
  activeAppContext: undefined,
}));
