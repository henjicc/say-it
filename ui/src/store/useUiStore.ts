import { create } from "zustand";

export type ViewKey = "dictation" | "assistant" | "subtitles" | "transcription" | "models" | "lexicon" | "history" | "settings" | "developer";
/** 窗口宽度决定的界面形态：窄窗口为简单模式，宽窗口为专业模式。 */
export type LayoutMode = "simple" | "pro";
export type DictationNodeKey = "basics" | "smart" | "rules" | "apps" | "shortcuts" | "orbGesture";
export type SubtitleNodeKey = "basics" | "translation" | "style" | "output";
export type AssistantActionKey = "translateSpeech" | "editSelection" | "ask";
/** 模型页中栏：用途、本地模型、某个供应商（`provider:<id>`）、插件、模型对比。 */
export type ModelsSectionKey = "usage" | "local" | "plugins" | "compare" | `provider:${string}`;
export type LexiconSectionKey = "hotwords" | "context";
export type SettingsSectionKey = "general" | "audio" | "keys" | "history" | "diagnostics";
export type SmartTextTabKey = "template" | "context" | "preview";

/** 设置搜索命中：跳到对应页面分区后，按 targetText 定位并短暂高亮。 */
export interface SearchHit {
  id: string;
  view: "models" | "settings" | "dictation";
  section: ModelsSectionKey | SettingsSectionKey | DictationNodeKey;
  targetText: string;
}

export interface SessionStatus {
  default_asr_provider?: string;
  defaultAsrProvider?: string;
  [key: string]: unknown;
}

interface UiState {
  view: ViewKey;
  setView: (view: ViewKey) => void;
  layoutMode: LayoutMode;
  setLayoutMode: (mode: LayoutMode) => void;
  dictationNode: DictationNodeKey;
  setDictationNode: (node: DictationNodeKey) => void;
  subtitleNode: SubtitleNodeKey;
  setSubtitleNode: (node: SubtitleNodeKey) => void;
  assistantAction: AssistantActionKey;
  setAssistantAction: (action: AssistantActionKey) => void;
  modelsSection: ModelsSectionKey;
  setModelsSection: (section: ModelsSectionKey) => void;
  lexiconSection: LexiconSectionKey;
  setLexiconSection: (section: LexiconSectionKey) => void;
  settingsSection: SettingsSectionKey;
  setSettingsSection: (section: SettingsSectionKey) => void;
  openModels: (section?: ModelsSectionKey) => void;
  searchHit: SearchHit | null;
  goToSearchHit: (hit: SearchHit) => void;
  clearSearchHit: (id: string) => void;
  openSettings: (section?: SettingsSectionKey) => void;
  smartTextTab: SmartTextTabKey;
  setSmartTextTab: (tab: SmartTextTabKey) => void;
  focusedShortcutProfileId: string | null;
  consumeFocusedShortcutProfile: () => void;
  openDictationShortcutSettings: (profileId?: string) => void;
  openAssistantSettings: (action?: AssistantActionKey) => void;
  openSubtitleShortcutSettings: () => void;
  aboutOpen: boolean;
  openAbout: () => void;
  closeAbout: () => void;
  session: SessionStatus | null;
  setSession: (status: SessionStatus | null) => void;
}

export const useUiStore = create<UiState>((set) => ({
  view: "dictation",
  setView: (view) => set({ view }),
  layoutMode: "pro",
  setLayoutMode: (layoutMode) => set({ layoutMode }),
  dictationNode: "basics",
  setDictationNode: (dictationNode) => set({ dictationNode }),
  subtitleNode: "basics",
  setSubtitleNode: (subtitleNode) => set({ subtitleNode }),
  assistantAction: "translateSpeech",
  setAssistantAction: (assistantAction) => set({ assistantAction }),
  modelsSection: "usage",
  setModelsSection: (modelsSection) => set({ modelsSection }),
  lexiconSection: "hotwords",
  setLexiconSection: (lexiconSection) => set({ lexiconSection }),
  settingsSection: "general",
  setSettingsSection: (settingsSection) => set({ settingsSection }),
  openModels: (section) => set((state) => ({ view: "models", modelsSection: section ?? state.modelsSection })),
  openSettings: (section) => set((state) => ({ view: "settings", settingsSection: section ?? state.settingsSection })),
  searchHit: null,
  goToSearchHit: (hit) => set(hit.view === "models"
    ? { view: "models", modelsSection: hit.section as ModelsSectionKey, searchHit: hit }
    : hit.view === "dictation"
      ? { view: "dictation", dictationNode: hit.section as DictationNodeKey, searchHit: hit }
      : { view: "settings", settingsSection: hit.section as SettingsSectionKey, searchHit: hit }),
  clearSearchHit: (id) => set((state) => (state.searchHit?.id === id ? { searchHit: null } : {})),
  smartTextTab: "template",
  setSmartTextTab: (smartTextTab) => set({ smartTextTab }),
  focusedShortcutProfileId: null,
  consumeFocusedShortcutProfile: () => set({ focusedShortcutProfileId: null }),
  openDictationShortcutSettings: (profileId) => set({
    view: "dictation",
    dictationNode: profileId ? "shortcuts" : "basics",
    focusedShortcutProfileId: profileId ?? null,
  }),
  openAssistantSettings: (action) => set((state) => ({
    view: "assistant",
    assistantAction: action ?? state.assistantAction,
    focusedShortcutProfileId: null,
  })),
  openSubtitleShortcutSettings: () => set({
    view: "subtitles",
    subtitleNode: "basics",
    focusedShortcutProfileId: null,
  }),
  aboutOpen: false,
  openAbout: () => set({ aboutOpen: true }),
  closeAbout: () => set({ aboutOpen: false }),
  session: null,
  setSession: (session) => set({ session }),
}));
