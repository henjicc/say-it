import { create } from "zustand";

export type ViewKey = "dictation" | "subtitles" | "transcription" | "settings";
export type SettingsTabKey = "provider" | "plugins" | "audio" | "disconnect" | "startup" | "mic" | "appearance" | "compare";

export interface SessionStatus {
  default_asr_provider?: string;
  defaultAsrProvider?: string;
  [key: string]: unknown;
}

interface UiState {
  view: ViewKey;
  setView: (view: ViewKey) => void;
  settingsTab: SettingsTabKey;
  setSettingsTab: (tab: SettingsTabKey) => void;
  aboutOpen: boolean;
  openAbout: () => void;
  closeAbout: () => void;
  session: SessionStatus | null;
  setSession: (status: SessionStatus | null) => void;
}

export const useUiStore = create<UiState>((set) => ({
  view: "dictation",
  setView: (view) => set({ view }),
  settingsTab: "provider",
  setSettingsTab: (settingsTab) => set({ settingsTab }),
  aboutOpen: false,
  openAbout: () => set({ aboutOpen: true }),
  closeAbout: () => set({ aboutOpen: false }),
  session: null,
  setSession: (session) => set({ session }),
}));
