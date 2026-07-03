export interface AsrModelOption {
  value: string;
  label: string;
}

export const DEFAULT_REALTIME_ASR_MODEL = "fun-asr-realtime-2026-02-28";
export const DEFAULT_FILE_ASR_MODEL = "fun-asr-flash-2026-06-15";

export const REALTIME_ASR_MODEL_OPTIONS: AsrModelOption[] = [
  { value: "fun-asr-realtime-2026-02-28", label: "Fun-ASR-Realtime 最新版" },
  { value: "fun-asr-realtime", label: "Fun-ASR-Realtime 稳定版" },
  { value: "qwen3-asr-flash-realtime-2026-02-10", label: "Qwen3-ASR-Flash-Realtime 最新版" },
  { value: "qwen3-asr-flash-realtime", label: "Qwen3-ASR-Flash-Realtime 稳定版" },
];

export const FILE_ASR_MODEL_OPTIONS: AsrModelOption[] = [
  { value: "fun-asr-flash-2026-06-15", label: "Fun-ASR-Flash" },
  { value: "fun-asr", label: "Fun-ASR" },
  { value: "qwen3-asr-flash-2026-02-10", label: "Qwen3-ASR-Flash 最新版" },
  { value: "qwen3-asr-flash", label: "Qwen3-ASR-Flash 稳定版" },
  { value: "qwen3-asr-flash-filetrans", label: "Qwen3-ASR-Flash-Filetrans" },
];

const REALTIME_MODEL_SET = new Set(REALTIME_ASR_MODEL_OPTIONS.map((option) => option.value));
const FILE_MODEL_SET = new Set(FILE_ASR_MODEL_OPTIONS.map((option) => option.value));

export function isSupportedRealtimeModel(model: string) {
  return REALTIME_MODEL_SET.has(model.trim());
}

export function isSupportedFileModel(model: string) {
  return FILE_MODEL_SET.has(model.trim());
}

export function isQwenRealtimeModel(model: string) {
  return model.trim().startsWith("qwen3-asr-flash-realtime");
}

export function isQwenFileModel(model: string) {
  return model.trim().startsWith("qwen3-asr-flash-filetrans");
}

export function isQwenShortAudioFileModel(model: string) {
  const value = model.trim();
  return value === "qwen3-asr-flash" || value === "qwen3-asr-flash-2026-02-10";
}

export function isFunAsrFlashFileModel(model: string) {
  return model.trim() === "fun-asr-flash-2026-06-15";
}

export function supportsFunAsrVocabularyId(model: string) {
  const value = model.trim();
  return value === "fun-asr" || value.startsWith("fun-asr-20") || value.startsWith("fun-asr-mtl");
}

export function supportsAlignmentTimestamps(model: string) {
  return model.trim() === "fun-asr" || isQwenFileModel(model);
}

export function realtimeModelSummary(model: string) {
  if (isQwenRealtimeModel(model)) {
    return "按百炼 realtime 协议调用；当前不复用 Fun-ASR 热词词表。";
  }
  return "支持现有 Fun-ASR 热词词表与实时高级参数。";
}

export function fileModelSummary(model: string) {
  if (isFunAsrFlashFileModel(model)) {
    return "短音频同步识别，适合 5 分钟以内文件；默认优先使用这个模型。";
  }
  if (isQwenShortAudioFileModel(model)) {
    return "短音频同步识别，适合 5 分钟以内文件；当前不复用 Fun-ASR 热词词表。";
  }
  if (isQwenFileModel(model)) {
    return "异步长音频转写，返回完整结果与时间戳；当前不复用 Fun-ASR 热词词表。";
  }
  return "异步录音转写；可手动填写当前 Fun-ASR 词表 ID。";
}
