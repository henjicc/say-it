import { create } from "zustand";
import { CMD, cmd } from "@/lib/tauri";

/** 字幕翻译与助手翻译输入共用的翻译设置（配置领域 `translation`），权威在 Rust `application::translation`。 */
export interface TranslationPrefs {
  /** dedicated：专用翻译模型 `model`；llm：大模型，`llmProviderId` 为 default 时跟随默认大模型。 */
  engine: "dedicated" | "llm";
  model: string;
  llmProviderId: string;
  llmModel: string;
}

const DEFAULT_PREFS: TranslationPrefs = { engine: "llm", model: "", llmProviderId: "default", llmModel: "" };

function normalize(value: Record<string, unknown> | undefined): TranslationPrefs {
  return {
    engine: value?.engine === "dedicated" ? "dedicated" : "llm",
    model: typeof value?.model === "string" ? value.model : DEFAULT_PREFS.model,
    llmProviderId: typeof value?.llmProviderId === "string" && value.llmProviderId ? value.llmProviderId : DEFAULT_PREFS.llmProviderId,
    llmModel: typeof value?.llmModel === "string" ? value.llmModel : DEFAULT_PREFS.llmModel,
  };
}

interface TranslationState {
  prefs: TranslationPrefs;
  save: (next: TranslationPrefs) => Promise<void>;
}

export const useTranslationStore = create<TranslationState>((set, get) => ({
  prefs: DEFAULT_PREFS,
  save: async (next) => {
    const previous = get().prefs;
    set({ prefs: next });
    try {
      await cmd(CMD.updateAppSettings, { domain: "translation", value: next });
    } catch (error) {
      set({ prefs: previous });
      throw error;
    }
  },
}));

export function hydrateTranslationPrefs(value: Record<string, unknown> | undefined) {
  useTranslationStore.setState({ prefs: normalize(value) });
}
