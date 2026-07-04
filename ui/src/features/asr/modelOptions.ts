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

// qwen3-asr-flash（同步短音频）不在这个列表里：它的响应里没有任何时间戳字段（无论流式与否），
// 生成不了字幕，字幕转写/文稿对齐都用不上，所以从识别模型下拉菜单里去掉，避免选中后没有字幕。
export const FILE_ASR_MODEL_OPTIONS: AsrModelOption[] = [
  { value: "fun-asr-flash-2026-06-15", label: "Fun-ASR-Flash" },
  { value: "fun-asr", label: "Fun-ASR" },
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
  return (
    value === "fun-asr" ||
    value.startsWith("fun-asr-20") ||
    value.startsWith("fun-asr-mtl") ||
    value.startsWith("paraformer")
  );
}

export function supportsAlignmentTimestamps(model: string) {
  return model.trim() === "fun-asr" || isQwenFileModel(model) || isFunAsrFlashFileModel(model);
}
