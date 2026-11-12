import { useState } from "react";
import { InspectorBody } from "@/components/layout/MasterDetail";
import { Group, Row, ROW_CONTROL_W } from "@/components/ui/Group";
import { ModelPicker } from "@/features/models/ModelPicker";
import { llmModelPickerOptions, llmModelValue } from "@/features/models/llmModelOptions";
import { UsageEditScope, useUsageIssue } from "@/features/models/usage";
import { useProviderStore } from "@/store/useProviderStore";
import { AsrModelRow } from "@/views/dictation/DictationSections";
import { OcrModelRow, SmartLlmRow } from "@/views/smart-text/ModelRows";
import { SubtitleAsrRow } from "@/views/subtitles/SubtitleSections";
import { SubtitleTranslationModelRow } from "@/views/SubtitleTranslationPanel";
import { TranscriptionModelRow } from "@/views/TranscriptionView";
import { ACTION_LABELS, AssistantModelRow, useUsageAssistantPrefs } from "@/views/VoiceAssistantPanel";
import type { AssistantActionKey } from "@/store/useUiStore";

/** 默认大模型：没单独选模型的功能（含「跟随默认大模型」与自动学习）使用它。 */
function DefaultLlmRow() {
  const profiles = useProviderStore((state) => state.profiles).filter(
    (profile) => profile.enabled && profile.capabilities.includes("llm"),
  );
  const defaults = useProviderStore((state) => state.defaults);
  const setDefault = useProviderStore((state) => state.setDefault);
  const updateConfig = useProviderStore((state) => state.updateConfig);
  const [error, setError] = useState("");
  const current = profiles.find((profile) => profile.id === defaults.llm);
  const selected = current ? llmModelValue(current.id, String(current.config?.model ?? "")) : "";
  const issue = useUsageIssue(current?.id, true, "llm");

  const change = async (value: string) => {
    const [providerId, model] = JSON.parse(value) as [string, string];
    setError("");
    try {
      await updateConfig(providerId, { model });
      if (providerId !== defaults.llm) await setDefault("llm", providerId);
    } catch (reason) {
      setError(`保存失败：${String(reason)}`);
    }
  };

  return (
    <>
      <Row
        label="默认"
        controlId="usage-general-llm"
        hint="没有单独选择模型的功能会使用它，例如选了「跟随默认大模型」的听写整理和助手。"
        message={error ? <span role="alert" className="text-[var(--color-err)]">{error}</span> : issue}
      >
        <div className={ROW_CONTROL_W}>
          <ModelPicker
            id="usage-general-llm"
            value={selected}
            options={llmModelPickerOptions(profiles, selected)}
            panelLabel="选择默认大模型"
            onChange={(value) => void change(value)}
          />
        </div>
      </Row>
    </>
  );
}

/**
 * 各功能用哪个模型，只在这里设置；下拉框里只列已填密钥、并在供应商里设为显示的模型。
 * 按模型种类分组，行名是功能名：同一类模型排在一起，方便一次换完。
 * 助手的语音翻译选了专用翻译引擎时用的是翻译模型，归到「翻译」组。
 */
export function UsagePanel() {
  const assistant = useUsageAssistantPrefs();
  const dedicatedTranslation = assistant.prefs?.translationEngine === "dedicated";
  const assistantRow = (action: AssistantActionKey) => assistant.prefs && (
    <AssistantModelRow action={action} label={ACTION_LABELS[action]} prefs={assistant.prefs} save={assistant.save} />
  );
  return (
    <UsageEditScope>
      <InspectorBody>
        <Group title="语音识别">
          <AsrModelRow label="听写" />
          <SubtitleAsrRow label="字幕" />
          <TranscriptionModelRow label="转写" />
        </Group>
        <Group title="大模型">
          <DefaultLlmRow />
          <SmartLlmRow label="听写整理" />
          {!dedicatedTranslation && assistantRow("translateSpeech")}
          {assistantRow("editSelection")}
          {assistantRow("ask")}
          {assistant.message && <p role="alert" className="px-3.5 py-3 text-xs text-[var(--color-err)]">{assistant.message}</p>}
        </Group>
        <Group title="翻译">
          <SubtitleTranslationModelRow label="字幕翻译" />
          {dedicatedTranslation && assistantRow("translateSpeech")}
        </Group>
        <Group title="文字识别"><OcrModelRow label="读屏" /></Group>
      </InspectorBody>
    </UsageEditScope>
  );
}
