import { modelInfo, useModelCatalogRevision } from "@/features/asr/modelRegistry";
import { useTranslationStore } from "@/store/useTranslationStore";

export interface TranslationLanguageOption {
  value: string;
  label: string;
  disabled?: boolean;
}

/** Qwen-MT 支持 92 种语种，这里收录常用子集；用代码而非英文名，API 两者都接受。 */
const COMMON_LANGUAGES: TranslationLanguageOption[] = [
  { value: "en", label: "英语" },
  { value: "zh", label: "简体中文" },
  { value: "zh_tw", label: "繁体中文" },
  { value: "ja", label: "日语" },
  { value: "ko", label: "韩语" },
  { value: "yue", label: "粤语" },
  { value: "ru", label: "俄语" },
  { value: "es", label: "西班牙语" },
  { value: "fr", label: "法语" },
  { value: "de", label: "德语" },
  { value: "it", label: "意大利语" },
  { value: "pt", label: "葡萄牙语" },
  { value: "nl", label: "荷兰语" },
  { value: "pl", label: "波兰语" },
  { value: "tr", label: "土耳其语" },
  { value: "vi", label: "越南语" },
  { value: "th", label: "泰语" },
  { value: "id", label: "印度尼西亚语" },
  { value: "ms", label: "马来语" },
  { value: "ar", label: "阿拉伯语" },
  { value: "hi", label: "印地语" },
  { value: "bn", label: "孟加拉语" },
  { value: "ur", label: "乌尔都语" },
  { value: "he", label: "希伯来语" },
  { value: "el", label: "希腊语" },
  { value: "sv", label: "瑞典语" },
  { value: "da", label: "丹麦语" },
  { value: "fi", label: "芬兰语" },
  { value: "cs", label: "捷克语" },
  { value: "ro", label: "罗马尼亚语" },
  { value: "uk", label: "乌克兰语" },
  { value: "hu", label: "匈牙利语" },
  { value: "km", label: "高棉语" },
  { value: "lo", label: "老挝语" },
];

const AUTO_LANGUAGE = { value: "auto", label: "自动检测" };
const EXTRA_LANGUAGE_LABELS: Record<string, string> = { nb: "挪威语", hr: "克罗地亚语", tl: "塔加洛语" };

function keepSelectedLanguage(options: TranslationLanguageOption[], value: string) {
  if (options.some((option) => option.value === value)) return options;
  const label = COMMON_LANGUAGES.find((option) => option.value === value)?.label ?? value;
  return [{ value, label: `${label}（当前模型不支持）`, disabled: true }, ...options];
}

export function useTranslationLanguageOptions(source: string, target: string): {
  sourceOptions: TranslationLanguageOption[];
  targetOptions: TranslationLanguageOption[];
} {
  useModelCatalogRevision();
  const prefs = useTranslationStore((state) => state.prefs);
  const supported = prefs.engine === "dedicated" ? modelInfo(prefs.model)?.translationLanguages : undefined;
  if (!supported) return { sourceOptions: [AUTO_LANGUAGE, ...COMMON_LANGUAGES], targetOptions: COMMON_LANGUAGES };

  const languages = supported.map((code) => {
    const value = code === "zh-Hant" ? "zh_tw" : code;
    const label = COMMON_LANGUAGES.find((option) => option.value === value)?.label ?? EXTRA_LANGUAGE_LABELS[value] ?? value;
    return { value, label };
  });
  return {
    sourceOptions: keepSelectedLanguage([AUTO_LANGUAGE, ...languages], source),
    targetOptions: keepSelectedLanguage(languages, target),
  };
}

export const DEFAULT_TRANSLATION_TARGET_LANG = "zh";
export const DEFAULT_TRANSLATION_SOURCE_LANG = "auto";
