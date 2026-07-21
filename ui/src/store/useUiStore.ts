import { create } from "zustand";

export type ViewKey = "dictation" | "subtitles" | "transcription" | "customization" | "settings";
export type CustomizationTabKey = "hotwords" | "context";
export type SettingsTabKey = "model" | "plugins" | "audio" | "general" | "compare" | "advanced";

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
  customizationTab: CustomizationTabKey;
  setCustomizationTab: (tab: CustomizationTabKey) => void;
  aboutOpen: boolean;
  openAbout: () => void;
  closeAbout: () => void;
  session: SessionStatus | null;
  setSession: (status: SessionStatus | null) => void;
}

export const useUiStore = create<UiState>((set) => ({
  view: "dictation",
  setView: (view) => set({ view }),
  settingsTab: "model",
  setSettingsTab: (settingsTab) => set({ settingsTab }),
  customizationTab: "hotwords",
  setCustomizationTab: (customizationTab) => set({ customizationTab }),
  aboutOpen: false,
  openAbout: () => set({ aboutOpen: true }),
  closeAbout: () => set({ aboutOpen: false }),
  session: null,
  setSession: (session) => set({ session }),
}));
