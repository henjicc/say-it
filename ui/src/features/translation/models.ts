export interface TranslationModelOption {
  value: string;
  label: string;
  /** 是否支持增量流式输出（每次只返回新增内容）；不支持的模型每次返回当前完整译文。 */
  supportsIncremental: boolean;
}

/** 关闭字幕翻译时 SubtitlePrefs.translationModel 的取值。 */
export const TRANSLATION_MODEL_NONE = "none";

export const TRANSLATION_MODEL_OPTIONS: TranslationModelOption[] = [
  { value: "qwen-mt-flash", label: "Qwen-MT-Flash（推荐，速度快）", supportsIncremental: true },
  { value: "qwen-mt-plus", label: "Qwen-MT-Plus（质量最高）", supportsIncremental: false },
  { value: "qwen-mt-lite", label: "Qwen-MT-Lite（延迟最低）", supportsIncremental: true },
];

const TRANSLATION_MODEL_SET = new Set(TRANSLATION_MODEL_OPTIONS.map((option) => option.value));

export function normalizeTranslationModel(model: string | undefined) {
  if (!model || model === TRANSLATION_MODEL_NONE) return TRANSLATION_MODEL_NONE;
  return TRANSLATION_MODEL_SET.has(model.trim()) ? model.trim() : TRANSLATION_MODEL_NONE;
}
