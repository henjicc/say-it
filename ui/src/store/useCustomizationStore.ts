import { create } from "zustand";
import { CMD, cmd } from "@/lib/tauri";
import { useProviderStore } from "@/store/useProviderStore";

/** 上下文模板中引用全局热词的变量，与后端 `application::customization` 保持一致。 */
export const HOTWORDS_PLACEHOLDER = "{{hotwords}}";
/** 智能处理提示词中引用全局上下文的变量。 */
export const GLOBAL_CONTEXT_PLACEHOLDER = "{{global_context}}";

export const MIN_HOTWORD_WEIGHT = 1;
export const MAX_HOTWORD_WEIGHT = 5;
export const DEFAULT_HOTWORD_WEIGHT = 4;
/** 供应商侧的上下文长度上限，与后端截断规则一致，用于界面提示。 */
export const MAX_CONTEXT_CHARS = 400;

export interface Hotword {
  text: string;
  weight: number;
}

export interface CustomizationPrefs {
  hotwords: Hotword[];
  contextTemplate: string;
}

export interface ProviderSyncResult {
  providerId: string;
  displayName: string;
  ok: boolean;
  message: string;
}

interface SyncResponse {
  results: ProviderSyncResult[];
  providers: unknown;
}

function defaults(): CustomizationPrefs {
  return { hotwords: [], contextTemplate: "" };
}

function normalize(value: unknown): CustomizationPrefs {
  const source = (value ?? {}) as Partial<CustomizationPrefs>;
  const hotwords = Array.isArray(source.hotwords) ? source.hotwords : [];
  return {
    hotwords: hotwords
      .filter((item): item is Hotword => !!item && typeof item.text === "string")
      .map((item) => ({
        text: item.text,
        weight: Number.isFinite(item.weight)
          ? Math.min(MAX_HOTWORD_WEIGHT, Math.max(MIN_HOTWORD_WEIGHT, Math.round(item.weight)))
          : DEFAULT_HOTWORD_WEIGHT,
      })),
    contextTemplate: typeof source.contextTemplate === "string" ? source.contextTemplate : "",
  };
}

/** 上下文预览：与后端 `render_context` 同规则，模板留空时退化为纯热词列表。 */
export function renderContextPreview(prefs: CustomizationPrefs): string {
  const hotwordsText = prefs.hotwords
    .map((item) => item.text.trim())
    .filter(Boolean)
    .join(" ");
  const template = prefs.contextTemplate.trim();
  const rendered = template ? template.split(HOTWORDS_PLACEHOLDER).join(hotwordsText) : hotwordsText;
  return [...rendered.trim()].slice(0, MAX_CONTEXT_CHARS).join("");
}

interface CustomizationState {
  prefs: CustomizationPrefs;
  syncResults: ProviderSyncResult[];
  patch: (partial: Partial<CustomizationPrefs>) => Promise<void>;
  syncToProviders: () => Promise<void>;
  pullFromProvider: (providerId: string) => Promise<void>;
  clearProviders: () => Promise<void>;
}

export const useCustomizationStore = create<CustomizationState>((set, get) => ({
  prefs: defaults(),
  syncResults: [],

  patch: async (partial) => {
    const next = { ...get().prefs, ...partial };
    await cmd(CMD.updateAppSettings, { domain: "customization", value: next });
    set({ prefs: next });
  },

  syncToProviders: async () => {
    const response = await cmd<SyncResponse>(CMD.customizationSyncProviders);
    useProviderStore.getState().hydrateCatalog(response.providers);
    set({ syncResults: response.results });
  },

  pullFromProvider: async (providerId) => {
    const prefs = await cmd<CustomizationPrefs>(CMD.customizationPullFromProvider, { providerId });
    set({ prefs: normalize(prefs), syncResults: [] });
  },

  clearProviders: async () => {
    const response = await cmd<SyncResponse>(CMD.customizationClearProviders);
    useProviderStore.getState().hydrateCatalog(response.providers);
    set({ syncResults: response.results });
  },
}));

export function hydrateCustomizationPrefs(value: Record<string, unknown> | undefined) {
  useCustomizationStore.setState({ prefs: normalize(value) });
}
