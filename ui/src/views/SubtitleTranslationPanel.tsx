import { InspectorBody } from "@/components/layout/MasterDetail";
import { Group, Row, ROW_CONTROL_W } from "@/components/ui/Group";
import { Select } from "@/components/ui/Input";
import { Tabs } from "@/components/ui/Tabs";
import {
  useSubtitleStore,
  type SubtitleTranslationLayout,
  type SubtitleTranslationOrder,
} from "@/store/useSubtitleStore";
import { TRANSLATION_MODEL_NONE } from "@/features/translation/models";
import { ModelPicker } from "@/features/models/ModelPicker";
import { UsageLinkRow, useOfferedTranslationOptions, useUsageEditable, useUsageIssue } from "@/features/models/usage";
import {
  TRANSLATION_SOURCE_LANGUAGE_OPTIONS,
  TRANSLATION_TARGET_LANGUAGE_OPTIONS,
} from "@/features/translation/languages";


/** 字幕翻译模型；选「无」即不翻译。 */
export function SubtitleTranslationModelRow() {
  const translationModel = useSubtitleStore((state) => state.prefs.translationModel);
  const setTranslationModel = useSubtitleStore((state) => state.setTranslationModel);
  const editable = useUsageEditable();
  const options = useOfferedTranslationOptions(translationModel);
  const selected = options.find((option) => option.value === translationModel);
  const issue = useUsageIssue(selected?.providerId, translationModel === TRANSLATION_MODEL_NONE || Boolean(selected), "translation");
  if (!editable) return <UsageLinkRow label="翻译模型" value={selected?.label ?? "不翻译"} message={issue} />;
  return (
    <Row label="翻译模型" hint="每句话识别完成后会自动翻译。部分模型会逐步显示译文，让你更早看到结果。" message={issue}>
      <div className={ROW_CONTROL_W}>
        <ModelPicker
          aria-label="翻译模型"
          value={translationModel}
          options={[{ value: TRANSLATION_MODEL_NONE, label: "无（不翻译）", providerId: "none", providerLabel: "不翻译" }, ...options]}
          panelLabel="选择翻译模型"
          onChange={(value) => void setTranslationModel(value)}
        />
      </div>
    </Row>
  );
}

export function SubtitleTranslationPanel() {
  const { prefs, patch } = useSubtitleStore();
  const enabled = prefs.translationModel !== TRANSLATION_MODEL_NONE;

  return (
    <InspectorBody>
      <Group>
        <SubtitleTranslationModelRow />
      </Group>

      {enabled && (
        <Group title="语种与显示">
          <Row label="源语言">
            <Select
              className={ROW_CONTROL_W}
              aria-label="源语言"
              value={prefs.translationSourceLang}
              onChange={(event) => patch({ translationSourceLang: event.target.value })}
            >
              {TRANSLATION_SOURCE_LANGUAGE_OPTIONS.map((option) => (
                <option key={option.value} value={option.value}>
                  {option.label}
                </option>
              ))}
            </Select>
          </Row>
          <Row label="目标语言">
            <Select
              className={ROW_CONTROL_W}
              aria-label="目标语言"
              value={prefs.translationTargetLang}
              onChange={(event) => patch({ translationTargetLang: event.target.value })}
            >
              {TRANSLATION_TARGET_LANGUAGE_OPTIONS.map((option) => (
                <option key={option.value} value={option.value}>
                  {option.label}
                </option>
              ))}
            </Select>
          </Row>
          <Row label="显示方式" hint="双语：同时显示原文和译文；仅译文：只显示翻译结果。">
            <Tabs<SubtitleTranslationLayout>
              variant="view"
              ariaLabel="译文显示方式"
              tabs={[{ key: "bilingual", label: "双语" }, { key: "translationOnly", label: "仅译文" }]}
              active={prefs.translationLayout}
              onChange={(translationLayout) => patch({ translationLayout })}
            />
          </Row>
          {prefs.translationLayout === "bilingual" && (
            <Row label="双语顺序">
              <Tabs<SubtitleTranslationOrder>
                variant="view"
                ariaLabel="双语顺序"
                tabs={[{ key: "sourceFirst", label: "原文在上" }, { key: "translationFirst", label: "译文在上" }]}
                active={prefs.translationOrder}
                onChange={(translationOrder) => patch({ translationOrder })}
              />
            </Row>
          )}
        </Group>
      )}
    </InspectorBody>
  );
}
