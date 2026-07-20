import { create } from "zustand";
import { CMD, cmd } from "@/lib/tauri";
import { useProviderStore, type ProviderProfile } from "@/store/useProviderStore";

/** 上下文模板中引用全局热词的变量，与后端 `application::customization` 保持一致。 */
export const HOTWORDS_PLACEHOLDER = "{{hotwords}}";
/** 智能处理提示词中引用全局上下文的变量。 */
export const GLOBAL_CONTEXT_PLACEHOLDER = "{{global_context}}";

export const MIN_HOTWORD_WEIGHT = 1;
export const MAX_HOTWORD_WEIGHT = 5;
export const DEFAULT_HOTWORD_WEIGHT = 4;
/** 供应商侧的上下文长度上限，与后端截断规则一致，用于界面提示。 */
export const MAX_CONTEXT_CHARS = 400;
/** 热词条数上限，与后端 `MAX_HOTWORDS` 一致。 */
export const MAX_HOTWORDS = 500;

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

/**
 * 上下文预览：与后端 `render_context` 同规则——上下文完全由模板决定，
 * 模板留空不下发，模板里没有 {{hotwords}} 就不带热词。
 */
export function renderContextPreview(prefs: CustomizationPrefs): string {
  const template = prefs.contextTemplate.trim();
  if (!template) return "";
  const hotwordsText = prefs.hotwords
    .map((item) => item.text.trim())
    .filter(Boolean)
    .join(" ");
  const rendered = template.split(HOTWORDS_PLACEHOLDER).join(hotwordsText);
  return [...rendered.trim()].slice(0, MAX_CONTEXT_CHARS).join("");
}

/** 热词自动同步到云端的防抖时长：编辑停顿这么久才推送，避免逐字触发厂商接口。 */
const AUTO_SYNC_DELAY_MS = 1500;

export type SyncState = "idle" | "pending" | "syncing" | "done" | "error";

/** 是否参与云端词表同步。判定与后端 `sync_targets` 保持一致，只在这里定义一次。 */
export function supportsHotwordSync(profile: ProviderProfile): boolean {
  return (
    profile.enabled &&
    (profile.capabilities.includes("customization") ||
      (profile.actions?.includes("manageHotwords") ?? false))
  );
}

/** 只比较真正会上传的内容：空白词条与权重之外的改动不该触发同步。 */
function hotwordsFingerprint(prefs: CustomizationPrefs): string {
  return JSON.stringify(
    prefs.hotwords
      .map((item) => ({ text: item.text.trim(), weight: item.weight }))
      .filter((item) => item.text.length > 0),
  );
}

interface CustomizationState {
  prefs: CustomizationPrefs;
  syncState: SyncState;
  syncMessage: string;
  syncResults: ProviderSyncResult[];
  patch: (partial: Partial<CustomizationPrefs>) => Promise<void>;
  pullFromProvider: (providerId: string) => Promise<void>;
  clearProviders: () => Promise<void>;
}

let autoSyncTimer: number | undefined;
let syncedFingerprint: string | undefined;

function cancelAutoSync() {
  if (autoSyncTimer !== undefined) window.clearTimeout(autoSyncTimer);
  autoSyncTimer = undefined;
}

/** 推送当前热词到所有支持云端词表的供应商。没有目标或词表为空时静默跳过。 */
async function runAutoSync() {
  const state = useCustomizationStore.getState();
  const fingerprint = hotwordsFingerprint(state.prefs);
  const targets = useProviderStore.getState().profiles.filter(supportsHotwordSync);
  if (fingerprint === "[]" || targets.length === 0) {
    syncedFingerprint = fingerprint;
    useCustomizationStore.setState({ syncState: "idle", syncMessage: "", syncResults: [] });
    return;
  }
  useCustomizationStore.setState({ syncState: "syncing", syncMessage: "正在同步到供应商…" });
  try {
    const response = await cmd<SyncResponse>(CMD.customizationSyncProviders);
    useProviderStore.getState().hydrateCatalog(response.providers);
    syncedFingerprint = fingerprint;
    const failed = response.results.filter((item) => !item.ok);
    useCustomizationStore.setState({
      syncResults: response.results,
      syncState: failed.length ? "error" : "done",
      syncMessage: failed.length ? "部分供应商同步失败" : "已同步到供应商",
    });
  } catch (error) {
    useCustomizationStore.setState({
      syncState: "error",
      syncMessage: `同步失败：${String(error)}`,
      syncResults: [],
    });
  }
}

function scheduleAutoSync() {
  const fingerprint = hotwordsFingerprint(useCustomizationStore.getState().prefs);
  if (fingerprint === syncedFingerprint) return;
  cancelAutoSync();
  useCustomizationStore.setState({ syncState: "pending", syncMessage: "待同步" });
  autoSyncTimer = window.setTimeout(() => {
    autoSyncTimer = undefined;
    void runAutoSync();
  }, AUTO_SYNC_DELAY_MS);
}

export const useCustomizationStore = create<CustomizationState>((set, get) => ({
  prefs: defaults(),
  syncState: "idle",
  syncMessage: "",
  syncResults: [],

  patch: async (partial) => {
    const next = { ...get().prefs, ...partial };
    await cmd(CMD.updateAppSettings, { domain: "customization", value: next });
    set({ prefs: next });
    if ("hotwords" in partial) scheduleAutoSync();
  },

  pullFromProvider: async (providerId) => {
    cancelAutoSync();
    const prefs = await cmd<CustomizationPrefs>(CMD.customizationPullFromProvider, { providerId });
    const next = normalize(prefs);
    // 拉下来的就是云端现状，不需要再推回去。
    syncedFingerprint = hotwordsFingerprint(next);
    set({ prefs: next, syncResults: [], syncState: "idle", syncMessage: "" });
  },

  clearProviders: async () => {
    cancelAutoSync();
    const response = await cmd<SyncResponse>(CMD.customizationClearProviders);
    useProviderStore.getState().hydrateCatalog(response.providers);
    // 云端已清空，本地热词与云端不再一致，下次编辑要重新推送。
    syncedFingerprint = undefined;
    set({ syncResults: response.results, syncState: "idle", syncMessage: "" });
  },
}));

export function hydrateCustomizationPrefs(value: Record<string, unknown> | undefined) {
  const prefs = normalize(value);
  // 启动时的已存配置视为与云端一致，不因为打开应用就触发一次同步。
  syncedFingerprint = hotwordsFingerprint(prefs);
  useCustomizationStore.setState({ prefs });
}
