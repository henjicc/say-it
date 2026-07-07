import { Field } from "@/components/ui/Field";
import { Select } from "@/components/ui/Input";
import { SettingsSection } from "@/components/ui/SettingsSection";
import { FormGrid } from "@/components/ui/FormGrid";
import {
  useSubtitleStore,
  type SubtitleTranslationLayout,
  type SubtitleTranslationOrder,
} from "@/store/useSubtitleStore";
import { TRANSLATION_MODEL_NONE, TRANSLATION_MODEL_OPTIONS } from "@/features/translation/models";
import {
  TRANSLATION_SOURCE_LANGUAGE_OPTIONS,
  TRANSLATION_TARGET_LANGUAGE_OPTIONS,
} from "@/features/translation/languages";

export function SubtitleTranslationPanel() {
  const { prefs, patch } = useSubtitleStore();
  const enabled = prefs.translationModel !== TRANSLATION_MODEL_NONE;

  return (
    <div className="flex flex-col gap-7">
      <SettingsSection title="字幕翻译">
        <p className="text-xs text-[var(--color-fg-subtle)]">
          每句字幕定稿后立即翻译，支持增量流式输出的模型会边生成边显示，尽量减少等待。
        </p>
        <FormGrid>
          <Field layout="row" label="翻译模型">
            <Select
              value={prefs.translationModel}
              onChange={(event) => patch({ translationModel: event.target.value })}
            >
              <option value={TRANSLATION_MODEL_NONE}>无（不翻译）</option>
              {TRANSLATION_MODEL_OPTIONS.map((option) => (
                <option key={option.value} value={option.value}>
                  {option.label}
                </option>
              ))}
            </Select>
          </Field>
        </FormGrid>
      </SettingsSection>

      {enabled && (
        <SettingsSection title="语种与显示">
          <FormGrid>
            <Field layout="row" label="源语言">
              <Select
                value={prefs.translationSourceLang}
                onChange={(event) => patch({ translationSourceLang: event.target.value })}
              >
                {TRANSLATION_SOURCE_LANGUAGE_OPTIONS.map((option) => (
                  <option key={option.value} value={option.value}>
                    {option.label}
                  </option>
                ))}
              </Select>
            </Field>
            <Field layout="row" label="目标语言">
              <Select
                value={prefs.translationTargetLang}
                onChange={(event) => patch({ translationTargetLang: event.target.value })}
              >
                {TRANSLATION_TARGET_LANGUAGE_OPTIONS.map((option) => (
                  <option key={option.value} value={option.value}>
                    {option.label}
                  </option>
                ))}
              </Select>
            </Field>
            <Field layout="row" label="显示方式">
              <Select
                value={prefs.translationLayout}
                onChange={(event) =>
                  patch({ translationLayout: event.target.value as SubtitleTranslationLayout })
                }
              >
                <option value="bilingual">双语（原文+译文）</option>
                <option value="translationOnly">仅译文</option>
              </Select>
            </Field>
            {prefs.translationLayout === "bilingual" && (
              <Field layout="row" label="双语顺序">
                <Select
                  value={prefs.translationOrder}
                  onChange={(event) =>
                    patch({ translationOrder: event.target.value as SubtitleTranslationOrder })
                  }
                >
                  <option value="sourceFirst">原文在上</option>
                  <option value="translationFirst">译文在上</option>
                </Select>
              </Field>
            )}
          </FormGrid>
        </SettingsSection>
      )}
    </div>
  );
}
