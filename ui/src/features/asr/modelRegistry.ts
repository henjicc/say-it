import { CMD, cmd } from "@/lib/tauri";

export interface ModelInfo {
  id: string; label: string; providerId: string; category: string; protocol: string;
  supportsVocabulary: boolean; supportsAlignmentTimestamps: boolean; scenes: string[];
  isDefaultRealtime: boolean; isDefaultFile: boolean; isQwenRealtime: boolean;
  isQwenFile: boolean; isQwenShortAudioFile: boolean; isFunasrFlashFile: boolean;
}
export interface AsrModelOption { value: string; label: string }
export interface ModelCatalogResponse {
  version: number; defaultRealtimeModel: string; defaultFileModel: string; models: ModelInfo[];
  providers: unknown;
}

let catalog: ModelCatalogResponse | null = null;

export async function loadModelCatalog(): Promise<ModelCatalogResponse> {
  catalog = await cmd<ModelCatalogResponse>(CMD.getModelCatalog);
  if (!catalog.models.length) throw new Error("后端模型目录为空");
  return catalog;
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
export const supportsAlignmentTimestamps = (id: string) => modelInfo(id)?.supportsAlignmentTimestamps ?? false;
export const isQwenRealtimeModel = (id: string) => modelInfo(id)?.isQwenRealtime ?? false;
export const isQwenFileModel = (id: string) => modelInfo(id)?.isQwenFile ?? false;
export const isQwenShortAudioFileModel = (id: string) => modelInfo(id)?.isQwenShortAudioFile ?? false;
export const isFunAsrFlashFileModel = (id: string) => modelInfo(id)?.isFunasrFlashFile ?? false;
export const defaultRealtimeModel = () => currentCatalog().defaultRealtimeModel;
export const defaultFileModel = () => currentCatalog().defaultFileModel;
