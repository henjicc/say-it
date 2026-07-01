import { create } from "zustand";

type Tone = "" | "ok" | "err";

interface DictationState {
  statusText: string;
  statusTone: Tone;
  latestText: string;
  log: string;
  recording: boolean;
  capturing: boolean;
  shortcutLabel: string;
  injectMethod: "paste" | "type";
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
}));
