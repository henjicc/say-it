import { InspectorBody } from "@/components/layout/MasterDetail";
import { Group, Row, ROW_CONTROL_W } from "@/components/ui/Group";
import { Select } from "@/components/ui/Input";
import { Tabs } from "@/components/ui/Tabs";
import {
  useSubtitleStore,
  type SubtitleTranslationLayout,
  type SubtitleTranslationOrder,
} from "@/store/useSubtitleStore";
import { Switch } from "@/components/ui/Switch";
import { TranslationModelRow } from "@/features/translation/TranslationModelRow";
import {
  TRANSLATION_SOURCE_LANGUAGE_OPTIONS,
  TRANSLATION_TARGET_LANGUAGE_OPTIONS,
} from "@/features/translation/languages";


export function SubtitleTranslationPanel() {
  const { prefs, patch } = useSubtitleStore();
  const enabled = prefs.translationEnabled;

  return (
    <InspectorBody>
      <Group>
        <Row label="翻译字幕" hint="每句话识别完成后自动翻译。部分模型会逐步显示译文，让你更早看到结果。">
          <Switch checked={enabled} onChange={(translationEnabled) => patch({ translationEnabled })} label="翻译字幕" />
        </Row>
        {enabled && <TranslationModelRow hint="字幕和助手的翻译输入共用这个模型。大模型的译文会比专用翻译模型晚一些出来。" />}
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
