// 本文件从模型注册表派生所有模型选项与能力判断，保持原有导出名称以确保消费方零改动。
import {
  optionsForScene,
  defaultRealtimeModel,
  defaultFileModel,
  supportsAlignmentTimestamps as registrySupportsAlignmentTimestamps,
  isQwenRealtimeModel as registryIsQwenRealtimeModel,
  isQwenFileModel as registryIsQwenFileModel,
  isQwenShortAudioFileModel as registryIsQwenShortAudioFileModel,
  isFunAsrFlashFileModel as registryIsFunAsrFlashFileModel,
  notifyModelCatalogUpdated,
  type AsrModelOption,
} from "./modelRegistry";
import builtinModels from "~shared/asr-models.json";

export type { AsrModelOption };

// Store 模块会早于异步启动桥求值，因此默认值必须同步来自共享注册表，不能先用空串占位。
// 启动桥随后仍会用后端目录（含插件模型）刷新这些值和选项。
export let DEFAULT_REALTIME_ASR_MODEL = builtinModels.find((model) => model.isDefaultRealtime)?.id ?? "";
export let DEFAULT_FILE_ASR_MODEL = builtinModels.find((model) => model.isDefaultFile)?.id ?? "";

// 模型下拉选项从注册表派生
export const REALTIME_ASR_MODEL_OPTIONS: AsrModelOption[] = [];
export const SUBTITLE_ASR_MODEL_OPTIONS: AsrModelOption[] = [];
export const FILE_ASR_MODEL_OPTIONS: AsrModelOption[] = [];
export const DICTATION_FILE_ASR_MODEL_OPTIONS: AsrModelOption[] = [];
export const DICTATION_ASR_MODEL_OPTIONS: AsrModelOption[] = [];

// 场景支持判断
const REALTIME_MODEL_SET = new Set<string>();
const FILE_MODEL_SET = new Set<string>();
const DICTATION_FILE_MODEL_SET = new Set<string>();
const DICTATION_MODEL_SET = new Set<string>();

function replace<T>(target: T[], values: T[]) { target.splice(0, target.length, ...values); }
function fillSet(target: Set<string>, values: AsrModelOption[]) { target.clear(); values.forEach((item) => target.add(item.value)); }

export function hydrateModelOptions() {
  DEFAULT_REALTIME_ASR_MODEL = defaultRealtimeModel();
  DEFAULT_FILE_ASR_MODEL = defaultFileModel();
  replace(REALTIME_ASR_MODEL_OPTIONS, optionsForScene("dictationRealtime"));
  replace(SUBTITLE_ASR_MODEL_OPTIONS, optionsForScene("subtitles"));
  replace(FILE_ASR_MODEL_OPTIONS, optionsForScene("transcription"));
  replace(DICTATION_FILE_ASR_MODEL_OPTIONS, optionsForScene("dictationFile"));
  replace(DICTATION_ASR_MODEL_OPTIONS, [...REALTIME_ASR_MODEL_OPTIONS, ...DICTATION_FILE_ASR_MODEL_OPTIONS]);
  fillSet(REALTIME_MODEL_SET, REALTIME_ASR_MODEL_OPTIONS); fillSet(FILE_MODEL_SET, FILE_ASR_MODEL_OPTIONS);
  fillSet(DICTATION_FILE_MODEL_SET, DICTATION_FILE_ASR_MODEL_OPTIONS); fillSet(DICTATION_MODEL_SET, DICTATION_ASR_MODEL_OPTIONS);
  notifyModelCatalogUpdated();
}

export function isSupportedRealtimeModel(model: string) {
  return REALTIME_MODEL_SET.has(model.trim());
}

export function isSupportedFileModel(model: string) {
  return FILE_MODEL_SET.has(model.trim());
}

export function isSupportedDictationModel(model: string) {
  return DICTATION_MODEL_SET.has(model.trim());
}

export function isDictationFileModel(model: string) {
  return DICTATION_FILE_MODEL_SET.has(model.trim());
}

// 协议族与能力判断，从注册表派生
export function isQwenRealtimeModel(model: string) {
  return registryIsQwenRealtimeModel(model);
}

export function isQwenFileModel(model: string) {
  return registryIsQwenFileModel(model);
}

export function isQwenShortAudioFileModel(model: string) {
  return registryIsQwenShortAudioFileModel(model);
}

export function isFunAsrFlashFileModel(model: string) {
  return registryIsFunAsrFlashFileModel(model);
}

export function supportsAlignmentTimestamps(model: string) {
  return registrySupportsAlignmentTimestamps(model);
}
