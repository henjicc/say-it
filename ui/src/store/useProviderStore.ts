import { create } from "zustand";
import { CMD, cmd } from "@/lib/tauri";

export type ProviderCapability = "asr";

export interface ProviderStatus {
  hasApiKey?: boolean;
}

export interface ProviderProfile {
  id: string;
  kind: string;
  displayName: string;
  authKind: string;
  capabilities: ProviderCapability[];
  enabled: boolean;
  isDefaultAsr?: boolean;
  status?: ProviderStatus;
  config?: Record<string, unknown>;
}

export interface ProviderDefaults {
  asr: string;
}

interface ProviderResponse {
  profiles?: ProviderProfile[];
  defaults?: ProviderDefaults;
}

interface ProviderState {
  profiles: ProviderProfile[];
  defaults: ProviderDefaults;
  overrides: Partial<Record<ProviderCapability, string>>;
  statusText: string;
  statusTone: "" | "ok" | "err";

  load: () => Promise<void>;
  setDefault: (capability: ProviderCapability, providerId: string) => Promise<void>;
  updateConfig: (providerId: string, config: Record<string, unknown>) => Promise<void>;
  saveFunasrHotwords: (hotwords: { text: string; weight: number }[]) => Promise<void>;
  clearFunasrHotwords: () => Promise<void>;
  setOverride: (capability: ProviderCapability, providerId: string) => void;
  effective: (capability: ProviderCapability) => string;
  optionsFor: (capability: ProviderCapability) => ProviderProfile[];
  labelFor: (providerId: string) => string;
}

const FALLBACK_DEFAULTS: ProviderDefaults = {
  asr: "funasr",
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
  },

  saveFunasrHotwords: async (hotwords) => {
    const response = await cmd<ProviderResponse>(CMD.funasrSaveHotwords, { hotwords });
    set({ ...normalize(response), statusText: "热词已保存到阿里云百炼。", statusTone: "ok" });
  },

  clearFunasrHotwords: async () => {
    const response = await cmd<ProviderResponse>(CMD.funasrClearHotwords);
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

  effective: (capability) => get().overrides[capability] || get().defaults[capability] || "funasr",

  optionsFor: (capability) =>
    get().profiles.filter((profile) => profile.enabled && profile.capabilities.includes(capability)),

  labelFor: (providerId) =>
    get().profiles.find((profile) => profile.id === providerId)?.displayName || providerId,
}));
