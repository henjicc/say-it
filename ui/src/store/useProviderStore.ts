import { create } from "zustand";
import { CMD, cmd } from "@/lib/tauri";

export type ProviderCapability = "asr" | "llm" | "translation" | "customization";

export interface ProviderStatus {
  hasApiKey?: boolean;
  configured?: boolean;
}

export interface ProviderProfile {
  id: string;
  kind: string;
  displayName: string;
  authKind: string;
  capabilities: ProviderCapability[];
  enabled: boolean;
  isDefaultAsr?: boolean;
  effectiveCapabilities?: ProviderCapability[];
  configFields?: { key: string; label: string; fieldType: string; secret: boolean }[];
  actions?: string[];
  status?: ProviderStatus;
  config?: Record<string, unknown>;
}

export interface ProviderDefaults {
  asr: string;
  // 预留给 LLM 后处理能力，当前未使用，空串表示未设置。
  llm: string;
  translation: string;
}

export interface ProviderResponse {
  profiles?: ProviderProfile[];
  defaults?: ProviderDefaults;
}

export type LlmReasoningEffort = "auto" | "zero" | "low" | "medium" | "high";
export type LlmModelSource = "remote" | "manual";
export type LlmModelAvailability = "available" | "missing" | "unknown";

export interface LlmModelConfig {
  name: string;
  source: LlmModelSource;
  availability: LlmModelAvailability;
  reasoningEffort: LlmReasoningEffort;
  temperature: number | null;
  maxTokens: number | null;
}

interface ProviderState {
  profiles: ProviderProfile[];
  defaults: ProviderDefaults;
  overrides: Partial<Record<ProviderCapability, string>>;
  statusText: string;
  statusTone: "" | "ok" | "err";
  hydrateCatalog: (response: unknown) => void;

  load: () => Promise<void>;
  setDefault: (capability: ProviderCapability, providerId: string) => Promise<void>;
  updateConfig: (providerId: string, config: Record<string, unknown>) => Promise<ProviderProfile>;
  addLlmProvider: (request: {
    adapter: string;
    displayName: string;
    model: string;
    apiKey: string;
    endpoint: string;
  }) => Promise<ProviderProfile>;
  refreshLlmModels: (providerId: string) => Promise<ProviderProfile>;
  removeLlmProvider: (providerId: string) => Promise<void>;
  saveFunasrHotwords: (hotwords: { text: string; weight: number }[]) => Promise<void>;
  syncFunasrHotwords: () => Promise<void>;
  clearFunasrHotwords: () => Promise<void>;
  saveHotwords: (providerId: string, hotwords: { text: string; weight: number }[]) => Promise<void>;
  syncHotwords: (providerId: string) => Promise<void>;
  clearHotwords: (providerId: string) => Promise<void>;
  setOverride: (capability: ProviderCapability, providerId: string) => void;
  effective: (capability: ProviderCapability) => string;
  optionsFor: (capability: ProviderCapability) => ProviderProfile[];
  labelFor: (providerId: string) => string;
}

const FALLBACK_DEFAULTS: ProviderDefaults = {
  asr: "",
  llm: "",
  translation: "",
};

function normalize(response: ProviderResponse): Pick<ProviderState, "profiles" | "defaults"> {
  return {
    profiles: response.profiles || [],
    defaults: { ...FALLBACK_DEFAULTS, ...(response.defaults || {}) },
  };
}

export const useProviderStore = create<ProviderState>((set, get) => ({
  profiles: [],
  defaults: FALLBACK_DEFAULTS,
  overrides: {},
  statusText: "",
  statusTone: "",
  hydrateCatalog: (response) => set(normalize(response as ProviderResponse)),

  load: async () => {
    try {
      const response = await cmd<ProviderResponse>(CMD.listProviders);
      const next = normalize(response);
      set({ ...next, statusText: "供应商配置已同步。", statusTone: "ok" });
    } catch (error) {
      set({ statusText: `供应商配置读取失败：${String(error)}`, statusTone: "err" });
    }
  },

  setDefault: async (capability, providerId) => {
    const response = await cmd<ProviderResponse>(CMD.setDefaultProvider, {
      request: { capability, providerId },
    });
    set({ ...normalize(response), statusText: "默认供应商已更新。", statusTone: "ok" });
  },

  updateConfig: async (providerId, config) => {
    const response = await cmd<ProviderResponse>(CMD.updateProviderConfig, {
      providerId,
      config,
    });
    set({ ...normalize(response), statusText: "供应商配置已保存。", statusTone: "ok" });
    const profile = response.profiles?.find((item) => item.id === providerId);
    if (!profile) throw new Error(`供应商 ${providerId} 不存在`);
    return profile;
  },

  addLlmProvider: async (request) => {
    const existingIds = new Set(get().profiles.map((profile) => profile.id));
    const response = await cmd<ProviderResponse>(CMD.addLlmProvider, { request });
    set({ ...normalize(response), statusText: "大语言模型已添加。", statusTone: "ok" });
    const profile = response.profiles?.find((item) => !existingIds.has(item.id));
    if (!profile) throw new Error("没有找到刚添加的大语言模型供应商");
    return profile;
  },

  refreshLlmModels: async (providerId) => {
    try {
      const response = await cmd<ProviderResponse>(CMD.refreshLlmModels, { providerId });
      set({ ...normalize(response), statusText: "模型列表已更新。", statusTone: "ok" });
      const profile = response.profiles?.find((item) => item.id === providerId);
      if (!profile) throw new Error(`供应商 ${providerId} 不存在`);
      return profile;
    } catch (error) {
      const response = await cmd<ProviderResponse>(CMD.listProviders);
      set({ ...normalize(response), statusText: `模型列表更新失败：${String(error)}`, statusTone: "err" });
      throw error;
    }
  },

  removeLlmProvider: async (providerId) => {
    const response = await cmd<ProviderResponse>(CMD.removeLlmProvider, { providerId });
    set({ ...normalize(response), statusText: "大语言模型已删除。", statusTone: "ok" });
  },

  saveFunasrHotwords: async (hotwords) => {
    const response = await cmd<ProviderResponse>(CMD.providerSaveHotwords, { providerId: "funasr", hotwords });
    set({ ...normalize(response), statusText: "热词已保存到阿里云百炼。", statusTone: "ok" });
  },

  syncFunasrHotwords: async () => {
    const response = await cmd<ProviderResponse>(CMD.providerSyncHotwords, { providerId: "funasr" });
    set({ ...normalize(response), statusText: "热词已从阿里云百炼同步。", statusTone: "ok" });
  },

  clearFunasrHotwords: async () => {
    const response = await cmd<ProviderResponse>(CMD.providerClearHotwords, { providerId: "funasr" });
    set({ ...normalize(response), statusText: "热词已清除。", statusTone: "ok" });
  },

  saveHotwords: async (providerId, hotwords) => {
    const response = await cmd<ProviderResponse>(CMD.providerSaveHotwords, { providerId, hotwords });
    set({ ...normalize(response), statusText: "热词已保存。", statusTone: "ok" });
  },

  syncHotwords: async (providerId) => {
    const response = await cmd<ProviderResponse>(CMD.providerSyncHotwords, { providerId });
    set({ ...normalize(response), statusText: "热词已同步。", statusTone: "ok" });
  },

  clearHotwords: async (providerId) => {
    const response = await cmd<ProviderResponse>(CMD.providerClearHotwords, { providerId });
    set({ ...normalize(response), statusText: "热词已清除。", statusTone: "ok" });
  },

  setOverride: (capability, providerId) => {
    set((state) => {
      const overrides = { ...state.overrides };
      if (!providerId || providerId === "default") delete overrides[capability];
      else overrides[capability] = providerId;
      return { overrides };
    });
  },

  effective: (capability) =>
    get().overrides[capability] ||
    get()
      .profiles.find((profile) => profile.effectiveCapabilities?.includes(capability))
      ?.id ||
    "",

  optionsFor: (capability) =>
    get().profiles.filter((profile) => profile.enabled && profile.capabilities.includes(capability)),

  labelFor: (providerId) =>
    get().profiles.find((profile) => profile.id === providerId)?.displayName || providerId,
}));
