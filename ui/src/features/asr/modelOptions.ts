export interface AsrModelOption {
  value: string;
  label: string;
}

export const DEFAULT_REALTIME_ASR_MODEL = "fun-asr-realtime";
export const DEFAULT_FILE_ASR_MODEL = "fun-asr";

export const REALTIME_ASR_MODEL_OPTIONS: AsrModelOption[] = [
  { value: "fun-asr-realtime", label: "Fun-ASR 稳定版" },
  { value: "fun-asr-realtime-2026-02-28", label: "Fun-ASR 最新快照" },
  { value: "fun-asr-realtime-2025-11-07", label: "Fun-ASR 2025-11-07" },
  { value: "qwen3-asr-flash-realtime", label: "Qwen3-ASR-Flash 实时版" },
  { value: "qwen3-asr-flash-realtime-2026-02-10", label: "Qwen3-ASR-Flash 最新快照" },
  { value: "paraformer-realtime-v2", label: "Paraformer v2" },
  { value: "paraformer-realtime-v1", label: "Paraformer v1" },
];

export const FILE_ASR_MODEL_OPTIONS: AsrModelOption[] = [
  { value: "fun-asr", label: "Fun-ASR 稳定版" },
  { value: "fun-asr-2025-11-07", label: "Fun-ASR 2025-11-07" },
  { value: "fun-asr-2025-08-25", label: "Fun-ASR 2025-08-25" },
  { value: "fun-asr-mtl", label: "Fun-ASR MTL 稳定版" },
  { value: "fun-asr-mtl-2025-08-25", label: "Fun-ASR MTL 2025-08-25" },
  { value: "qwen3-asr-flash-filetrans", label: "Qwen3-ASR-Flash-Filetrans 稳定版" },
  { value: "qwen3-asr-flash-filetrans-2025-11-17", label: "Qwen3-ASR-Flash-Filetrans 2025-11-17" },
  { value: "paraformer-v2", label: "Paraformer v2" },
  { value: "paraformer-v1", label: "Paraformer v1" },
  { value: "paraformer-mtl-v1", label: "Paraformer MTL v1" },
];

export function isQwenRealtimeModel(model: string) {
  return model.trim().startsWith("qwen3-asr-flash-realtime");
}

export function isQwenFileModel(model: string) {
  return model.trim().startsWith("qwen3-asr-flash-filetrans");
}

export function isFunAsrFileModel(model: string) {
  return model.trim().startsWith("fun-asr");
}

export function realtimeModelSummary(model: string) {
  if (isQwenRealtimeModel(model)) {
    return "偏快、偏口语理解；当前不复用 Fun-ASR 热词词表。";
  }
  if (model.trim().startsWith("paraformer")) {
    return "兼容现有实时链路；当前热词面板仍只管理 Fun-ASR 词表。";
  }
  return "支持现有 Fun-ASR 热词词表与实时高级参数。";
}

export function fileModelSummary(model: string) {
  if (isQwenFileModel(model)) {
    return "异步长音频转写，返回完整结果与时间戳；当前不复用 Fun-ASR 热词词表。";
  }
  if (model.trim().startsWith("paraformer")) {
    return "异步录音转写；当前热词面板仍只管理 Fun-ASR 词表。";
  }
  return "异步录音转写；可手动填写当前 Fun-ASR 词表 ID。";
}
