import { create } from "zustand";

export type SubtitleSource = "microphone" | "system";
export type SubtitleAnchor = "top" | "center" | "bottom";
export type SubtitleMode = "scroll" | "replace";

export interface SubtitlePrefs {
  source: SubtitleSource;
  mode: SubtitleMode;
  fontFamily: string;
  /** 字号，屏幕高度的百分比 */
  fontSizePercent: number;
  lineCount: number;
  /** 字幕框最大宽度，屏幕宽度的百分比 */
  widthPercent: number;
  anchor: SubtitleAnchor;
  /** 相对锚点的位置偏移，屏幕高度的百分比 */
  offsetYPercent: number;
  textColor: string;
  backgroundColor: string;
  backgroundOpacity: number;
  rounded: number;
}

type Tone = "" | "ok" | "err";

interface SubtitleState {
  prefs: SubtitlePrefs;
  running: boolean;
  statusText: string;
  statusTone: Tone;
  latestText: string;
  capturing: boolean;
  shortcutLabel: string;
  patch: (partial: Partial<SubtitlePrefs>) => void;
  setRuntime: (
    partial: Partial<
      Pick<SubtitleState, "running" | "statusText" | "statusTone" | "latestText" | "capturing" | "shortcutLabel">
    >,
  ) => void;
}

const SUBTITLE_PREFS_KEY = "sayItSubtitlePrefs";

const defaults = (): SubtitlePrefs => ({
  source: "microphone",
  mode: "replace",
  fontFamily: "Microsoft YaHei",
  fontSizePercent: 2.6,
  lineCount: 1,
  widthPercent: 46,
  anchor: "bottom",
  offsetYPercent: 6,
  textColor: "#ffffff",
  backgroundColor: "#05070a",
  backgroundOpacity: 72,
  rounded: 18,
});

function clampPrefs(prefs: SubtitlePrefs): SubtitlePrefs {
  return {
    ...prefs,
    fontSizePercent: Math.min(6, Math.max(1.5, Number(prefs.fontSizePercent) || 2.6)),
    lineCount: Math.min(4, Math.max(1, Math.round(Number(prefs.lineCount) || 1))),
    widthPercent: Math.min(70, Math.max(20, Number(prefs.widthPercent) || 46)),
    offsetYPercent: Math.min(20, Math.max(-17, Number(prefs.offsetYPercent) || 6)),
    backgroundOpacity: Math.min(100, Math.max(0, Number(prefs.backgroundOpacity) || 72)),
    rounded: Math.min(36, Math.max(0, Number(prefs.rounded) || 18)),
  };
}

function readStored(): SubtitlePrefs {
  const base = defaults();
  try {
    const raw = localStorage.getItem(SUBTITLE_PREFS_KEY);
    if (raw) Object.assign(base, JSON.parse(raw));
  } catch {
    /* noop */
  }
  return clampPrefs(base);
}

function persist(prefs: SubtitlePrefs) {
  try {
    localStorage.setItem(SUBTITLE_PREFS_KEY, JSON.stringify(prefs));
  } catch {
    /* noop */
  }
}

export const useSubtitleStore = create<SubtitleState>((set, get) => ({
  prefs: readStored(),
  running: false,
  statusText: "实时字幕未开启",
  statusTone: "",
  latestText: "",
  capturing: false,
  shortcutLabel: "",
  patch: (partial) => {
    const next = clampPrefs({ ...get().prefs, ...partial });
    persist(next);
    set({ prefs: next });
  },
  setRuntime: (partial) => set(partial),
}));

