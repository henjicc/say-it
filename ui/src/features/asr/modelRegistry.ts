import { useSyncExternalStore } from "react";
import { CMD, cmd } from "@/lib/tauri";

export interface ModelInfo {
  id: string; label: string; providerId: string; category: string; protocol: string;
  supportsVocabulary: boolean; supportsAlignmentTimestamps: boolean; scenes: string[];
  isDefaultRealtime: boolean; isDefaultFile: boolean; isQwenRealtime: boolean;
  isQwenFile: boolean; isQwenShortAudioFile: boolean; isFunasrFlashFile: boolean;
}
export interface AsrModelOption { value: string; label: string }
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
export function optionsForScene(scene: string): AsrModelOption[] {
  return currentCatalog().models.filter((item) => item.scenes.includes(scene)).map((item) => ({
    value: item.id,
    label: scene === "dictationFile" && item.category === "file" ? `${item.label}（非实时）` : item.label,
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
export const isQwenRealtimeModel = (id: string) => modelInfo(id)?.isQwenRealtime ?? false;
export const isQwenFileModel = (id: string) => modelInfo(id)?.isQwenFile ?? false;
export const isQwenShortAudioFileModel = (id: string) => modelInfo(id)?.isQwenShortAudioFile ?? false;
export const isFunAsrFlashFileModel = (id: string) => modelInfo(id)?.isFunasrFlashFile ?? false;
export const defaultRealtimeModel = () => currentCatalog().defaultRealtimeModel;
export const defaultFileModel = () => currentCatalog().defaultFileModel;
