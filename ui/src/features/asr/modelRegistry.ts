// 从仓库根的 shared/asr-models.json 导入模型注册表（Vite 会内联进 bundle）
import registryData from "~shared/asr-models.json";

export interface ModelInfo {
  id: string;
  label: string;
  providerKind: string;
  category: string;
  protocol: string;
  supportsVocabulary: boolean;
  supportsAlignmentTimestamps: boolean;
  scenes: string[];
  isDefaultRealtime: boolean;
  isDefaultFile: boolean;
}

export interface AsrModelOption {
  value: string;
  label: string;
}

// 类型安全的注册表访问
const REGISTRY: ModelInfo[] = registryData as ModelInfo[];

/** 非实时（文件）模型在听写下拉里需要标注的后缀，用于和实时模型区分。 */
const NON_REALTIME_SUFFIX = "（非实时）";

/**
 * 从注册表查询模型信息；表外模型返回 undefined。
 */
export function modelInfo(id: string): ModelInfo | undefined {
  const normalized = id.trim();
  return REGISTRY.find((info) => info.id === normalized);
}

/**
 * 按场景过滤模型，返回下拉选项列表。
 *
 * 听写场景（dictationFile）会把实时与非实时模型混在同一个下拉里，因此这里根据 `category`
 * 字段为非实时（file）模型的 label 追加"（非实时）"后缀加以区分；录音识别（transcription）
 * 场景全是文件模型，无需标注，直接用注册表基础 label。
 */
export function optionsForScene(scene: string): AsrModelOption[] {
  const annotateNonRealtime = scene === "dictationFile";
  return REGISTRY.filter((info) => info.scenes.includes(scene)).map((info) => ({
    value: info.id,
    label:
      annotateNonRealtime && info.category === "file"
        ? `${info.label}${NON_REALTIME_SUFFIX}`
        : info.label,
  }));
}

/**
 * 判断模型是否支持对齐时间戳（文稿对齐场景需要）。
 * 表外模型返回 false。
 */
export function supportsAlignmentTimestamps(model: string): boolean {
  return modelInfo(model)?.supportsAlignmentTimestamps ?? false;
}

/**
 * 判断模型是否为 Qwen 实时识别协议。
 * 表内模型查表，表外模型按前缀兜底（与 Rust 侧一致）。
 */
export function isQwenRealtimeModel(model: string): boolean {
  const info = modelInfo(model);
  if (info) {
    return info.protocol === "qwen-realtime";
  }
  // 表外模型前缀兜底
  return model.trim().startsWith("qwen3-asr-flash-realtime");
}

/**
 * 判断模型是否为 Qwen 文件转写协议（filetrans）。
 * 表内模型查表，表外模型按前缀兜底。
 */
export function isQwenFileModel(model: string): boolean {
  const info = modelInfo(model);
  if (info) {
    return info.protocol === "file-async-oss" && info.id.startsWith("qwen3-asr-flash-filetrans");
  }
  // 表外模型前缀兜底
  return model.trim().startsWith("qwen3-asr-flash-filetrans");
}

/**
 * 判断模型是否为 Qwen 短音频同步识别模型。
 * 表内模型查表，表外模型返回 false。
 */
export function isQwenShortAudioFileModel(model: string): boolean {
  const info = modelInfo(model);
  if (info) {
    return info.protocol === "file-sync-qwen";
  }
  // 表外模型前缀兜底（保持与原逻辑一致）
  const value = model.trim();
  return value === "qwen3-asr-flash" || value === "qwen3-asr-flash-2026-02-10";
}

/**
 * 判断模型是否为 Fun-ASR-Flash 文件识别模型。
 * 表内模型查表，表外模型返回 false。
 */
export function isFunAsrFlashFileModel(model: string): boolean {
  const info = modelInfo(model);
  if (info) {
    return info.protocol === "file-sync-funasr-flash";
  }
  // 表外模型返回 false
  return model.trim() === "fun-asr-flash-2026-06-15";
}

/**
 * 获取默认的实时识别模型 ID。
 */
export function defaultRealtimeModel(): string {
  return REGISTRY.find((info) => info.isDefaultRealtime)?.id ?? "fun-asr-realtime-2026-02-28";
}

/**
 * 获取默认的文件识别模型 ID。
 */
export function defaultFileModel(): string {
  return REGISTRY.find((info) => info.isDefaultFile)?.id ?? "fun-asr-flash-2026-06-15";
}
