export interface TranslationModelOption {
  value: string;
  label: string;
  /** 是否支持增量流式输出（每次只返回新增内容）；不支持的模型每次返回当前完整译文。 */
  supportsIncremental: boolean;
}

/** 内置 Qwen-MT 翻译模型使用阿里云百炼的密钥。 */
export const TRANSLATION_PROVIDER_ID = "bailian";

export const TRANSLATION_MODEL_OPTIONS: TranslationModelOption[] = [
  { value: "qwen-mt-flash", label: "Qwen-MT-Flash（推荐，速度快）", supportsIncremental: true },
  { value: "qwen-mt-plus", label: "Qwen-MT-Plus（质量最高）", supportsIncremental: false },
  { value: "qwen-mt-lite", label: "Qwen-MT-Lite（延迟最低）", supportsIncremental: true },
];
