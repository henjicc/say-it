import { useSyncExternalStore } from "react";
import { CMD, cmd } from "@/lib/tauri";

export interface ModelInfo {
  id: string; label: string; providerId: string; category: string; protocol: string;
  supportsVocabulary: boolean; supportsContext: boolean; supportsAlignmentTimestamps: boolean;
  emitsPartialResults: boolean; scenes: string[];
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
/**
 * 出字方式标注。语音输入下拉把实时与非实时模型混在一起，实时字幕下拉里也可能混入
 * 无中间结果的整句模型，用户必须能在选之前就看出差别：
 *
 * - 真流式（边说边出字）：不加后缀
 * - 整句模型：走实时会话，但说完一句才整句出字，没有中间态
 * - 非实时：停止后才开始识别
 */
function outputModeSuffix(item: ModelInfo, scene: string): string {
  if (scene === "dictationFile" && item.category === "file") return "（非实时）";
  if ((scene === "dictationRealtime" || scene === "subtitles") && !item.emitsPartialResults) {
    return "（整句）";
  }
  return "";
}
export function optionsForScene(scene: string): AsrModelOption[] {
  return currentCatalog().models.filter((item) => item.scenes.includes(scene)).map((item) => ({
    value: item.id,
    label: `${item.label}${outputModeSuffix(item, scene)}`,
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
