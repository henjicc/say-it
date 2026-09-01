import { useSyncExternalStore } from "react";
import { CMD, cmd } from "@/lib/tauri";

export interface ModelInfo {
  id: string; label: string; providerId: string; category: string; protocol: string;
  supportsVocabulary: boolean; supportsContext: boolean; supportsAlignmentTimestamps: boolean;
  emitsPartialResults: boolean; scenes: string[];
  isDefaultRealtime: boolean; isDefaultFile: boolean; isQwenRealtime: boolean;
  isQwenFile: boolean; isQwenShortAudioFile: boolean; isFunasrFlashFile: boolean;
}
export type AsrModelMode = "realtime" | "nonRealtime";
export interface AsrModelOption {
  value: string;
  label: string;
  providerId: string;
  providerLabel?: string;
  mode?: AsrModelMode;
}
export interface OcrModelOption extends AsrModelOption {
  providerId: string;
  remote: boolean;
}
interface CatalogProvider {
  id: string;
  kind: string;
  displayName: string;
  capabilities: string[];
  enabled: boolean;
}
export interface ModelCatalogResponse {
  version: number; defaultRealtimeModel: string; defaultFileModel: string; models: ModelInfo[];
  providers: { profiles?: CatalogProvider[]; defaults?: Record<string, string> };
}

let catalog: ModelCatalogResponse | null = null;
let catalogRevision = 0;
const catalogListeners = new Set<() => void>();

export async function loadModelCatalog(): Promise<ModelCatalogResponse> {
  catalog = await cmd<ModelCatalogResponse>(CMD.getModelCatalog);
  if (!catalog.models.length) throw new Error("后端模型目录为空");
  return catalog;
}

export function notifyModelCatalogUpdated() {
  catalogRevision += 1;
  catalogListeners.forEach((listener) => listener());
}

export function useModelCatalogRevision() {
  return useSyncExternalStore(
    (listener) => {
      catalogListeners.add(listener);
      return () => catalogListeners.delete(listener);
    },
    () => catalogRevision,
    () => catalogRevision,
  );
}
export function currentCatalog(): ModelCatalogResponse {
  if (!catalog) throw new Error("模型目录尚未加载");
  return catalog;
}
export function modelInfo(id: string) { return catalog?.models.find((item) => item.id === id.trim()); }
export function asrModelMode(item: Pick<ModelInfo, "category">): AsrModelMode | undefined {
  if (item.category === "realtime") return "realtime";
  if (item.category === "file") return "nonRealtime";
  return undefined;
}
export function asrModelModeLabel(mode: AsrModelMode): string {
  return mode === "realtime" ? "实时" : "非实时";
}
export function asrModelDisplayLabel(item: Pick<ModelInfo, "label" | "category">): string {
  const mode = asrModelMode(item);
  if (!mode) return item.label;
  const baseLabel = item.label.replace(/\s*[（(](?:实时|非实时)[）)]\s*$/u, "");
  return `${baseLabel}（${asrModelModeLabel(mode)}）`;
}
export function optionsForScene(scene: string): AsrModelOption[] {
  const current = currentCatalog();
  const providerNames = new Map(
    (current.providers.profiles || []).map((provider) => [provider.id, provider.displayName]),
  );
  return current.models.filter((item) => item.scenes.includes(scene)).map((item) => ({
    value: item.id,
    label: asrModelDisplayLabel(item),
    providerId: item.providerId,
    providerLabel: providerNames.get(item.providerId) || item.providerId,
    mode: asrModelMode(item),
  }));
}
export function ocrOptionsForScene(scene: string): OcrModelOption[] {
  const current = currentCatalog();
  const providers = (current.providers.profiles || []).filter(
    (provider) => provider.enabled && provider.capabilities.includes("ocr"),
  );
  const byProvider = new Map(providers.map((provider) => [provider.id, provider]));
  const explicit = current.models
    .filter((item) => item.category === "ocr" && item.scenes.includes(scene))
    .map((item) => {
      const provider = byProvider.get(item.providerId);
      return {
        value: item.id,
        label: item.label,
        providerId: item.providerId,
        remote: provider?.kind.startsWith("plugin:") ?? false,
      };
    });
  const explicitProviders = new Set(explicit.map((option) => option.providerId));
  const implicit = providers
    .filter((provider) => !explicitProviders.has(provider.id))
    .map((provider) => ({
      value: provider.id,
      label: provider.displayName,
      providerId: provider.id,
      remote: provider.kind.startsWith("plugin:"),
    }));
  return [...implicit, ...explicit];
}
export const supportsAlignmentTimestamps = (id: string) => modelInfo(id)?.supportsAlignmentTimestamps ?? false;
/** 场景内可用于文稿对齐的模型名，供提示文案直接列举，避免硬编码供应商名。 */
export function timestampCapableLabels(scene: string): string[] {
  return currentCatalog()
    .models.filter((item) => item.scenes.includes(scene) && item.supportsAlignmentTimestamps)
    .map((item) => item.label);
}
export const isQwenRealtimeModel = (id: string) => modelInfo(id)?.isQwenRealtime ?? false;
export const isQwenFileModel = (id: string) => modelInfo(id)?.isQwenFile ?? false;
export const isQwenShortAudioFileModel = (id: string) => modelInfo(id)?.isQwenShortAudioFile ?? false;
export const isFunAsrFlashFileModel = (id: string) => modelInfo(id)?.isFunasrFlashFile ?? false;
export const defaultRealtimeModel = () => currentCatalog().defaultRealtimeModel;
export const defaultFileModel = () => currentCatalog().defaultFileModel;
