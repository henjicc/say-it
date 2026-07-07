export interface TranslationModelOption {
  value: string;
  label: string;
  /** 是否支持增量流式输出（每次只返回新增内容）；不支持的模型每次返回当前完整译文。 */
  supportsIncremental: boolean;
}

/** 关闭字幕翻译时 SubtitlePrefs.translationModel 的取值。 */
export const TRANSLATION_MODEL_NONE = "";

export const TRANSLATION_MODEL_OPTIONS: TranslationModelOption[] = [
  { value: "qwen-mt-flash", label: "qwen-mt-flash（推荐，速度快）", supportsIncremental: true },
  { value: "qwen-mt-plus", label: "qwen-mt-plus（质量最高）", supportsIncremental: false },
  { value: "qwen-mt-lite", label: "qwen-mt-lite（延迟最低）", supportsIncremental: true },
];

const TRANSLATION_MODEL_SET = new Set(TRANSLATION_MODEL_OPTIONS.map((option) => option.value));

export function isSupportedTranslationModel(model: string) {
  return model === TRANSLATION_MODEL_NONE || TRANSLATION_MODEL_SET.has(model.trim());
}
