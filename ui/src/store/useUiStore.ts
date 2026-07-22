import { create } from "zustand";

export type ViewKey = "dictation" | "subtitles" | "transcription" | "customization" | "settings";
export type CustomizationTabKey = "hotwords" | "context";
export type SettingsTabKey = "model" | "plugins" | "audio" | "general" | "keys" | "compare" | "advanced";
export type DictationTabKey = "basic" | "local" | "smart" | "apps" | "debug";
export type SceneRulesTabKey = "apps" | "shortcuts";
export type SubtitleTabKey = "general" | "style" | "translation" | "obs";

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
  dictationTab: DictationTabKey;
  setDictationTab: (tab: DictationTabKey) => void;
  sceneRulesTab: SceneRulesTabKey;
  setSceneRulesTab: (tab: SceneRulesTabKey) => void;
  subtitleTab: SubtitleTabKey;
  setSubtitleTab: (tab: SubtitleTabKey) => void;
  focusedShortcutProfileId: string | null;
  consumeFocusedShortcutProfile: () => void;
  openDictationShortcutSettings: (profileId?: string) => void;
  openSubtitleShortcutSettings: () => void;
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
  dictationTab: "basic",
  setDictationTab: (dictationTab) => set({ dictationTab }),
  sceneRulesTab: "apps",
  setSceneRulesTab: (sceneRulesTab) => set({ sceneRulesTab }),
  subtitleTab: "general",
  setSubtitleTab: (subtitleTab) => set({ subtitleTab }),
  focusedShortcutProfileId: null,
  consumeFocusedShortcutProfile: () => set({ focusedShortcutProfileId: null }),
  openDictationShortcutSettings: (profileId) => set(profileId
    ? {
      view: "dictation",
      dictationTab: "apps",
      sceneRulesTab: "shortcuts",
      focusedShortcutProfileId: profileId,
    }
    : {
      view: "dictation",
      dictationTab: "basic",
      focusedShortcutProfileId: null,
    }),
  openSubtitleShortcutSettings: () => set({
    view: "subtitles",
    subtitleTab: "general",
    focusedShortcutProfileId: null,
  }),
  customizationTab: "hotwords",
  setCustomizationTab: (customizationTab) => set({ customizationTab }),
  aboutOpen: false,
  openAbout: () => set({ aboutOpen: true }),
  closeAbout: () => set({ aboutOpen: false }),
  session: null,
  setSession: (session) => set({ session }),
}));
