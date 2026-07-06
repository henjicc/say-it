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
  type AsrModelOption,
} from "./modelRegistry";

export type { AsrModelOption };

// 默认模型从注册表派生
export const DEFAULT_REALTIME_ASR_MODEL = defaultRealtimeModel();
export const DEFAULT_FILE_ASR_MODEL = defaultFileModel();

// 模型下拉选项从注册表派生
export const REALTIME_ASR_MODEL_OPTIONS: AsrModelOption[] = optionsForScene("dictationRealtime");
export const FILE_ASR_MODEL_OPTIONS: AsrModelOption[] = optionsForScene("transcription");
export const DICTATION_FILE_ASR_MODEL_OPTIONS: AsrModelOption[] = optionsForScene("dictationFile");
export const DICTATION_ASR_MODEL_OPTIONS: AsrModelOption[] = [
  ...REALTIME_ASR_MODEL_OPTIONS,
  ...DICTATION_FILE_ASR_MODEL_OPTIONS,
];

// 场景支持判断
const REALTIME_MODEL_SET = new Set(REALTIME_ASR_MODEL_OPTIONS.map((option) => option.value));
const FILE_MODEL_SET = new Set(FILE_ASR_MODEL_OPTIONS.map((option) => option.value));
const DICTATION_FILE_MODEL_SET = new Set(
  DICTATION_FILE_ASR_MODEL_OPTIONS.map((option) => option.value),
);
const DICTATION_MODEL_SET = new Set(DICTATION_ASR_MODEL_OPTIONS.map((option) => option.value));

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
