import { create } from "zustand";

type Tone = "" | "ok" | "err";

export type ActiveAppContextStatus =
  | "captured"
  | "empty"
  | "blocked"
  | "timedOut"
  | "unsupported"
  | "failed";

export interface ActiveAppContextSummary {
  status: ActiveAppContextStatus;
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
  capturing: boolean;
  shortcutLabel: string;
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
  capturing: false,
  shortcutLabel: "",
  injectMethod: "paste",
  pressHoldMode: false,
  activeAppContext: undefined,
}));
