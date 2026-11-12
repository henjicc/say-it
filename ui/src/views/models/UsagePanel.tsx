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
import { TranslationModelRow } from "@/features/translation/TranslationModelRow";
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
        label="默认大模型"
        controlId="usage-general-llm"
        hint="没有单独选择模型的功能会使用它，例如选了「跟随默认大模型」的语音输入整理和助手。"
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
 * 分「识别」「文本处理」两组，行名写用在什么场景，对模型的要求放进悬停说明。
 * 翻译只有一个，字幕翻译和助手的翻译输入共用；它可以用大模型，所以归在文本处理。
 */
export function UsagePanel() {
  const assistant = useUsageAssistantPrefs();
  const assistantRow = (action: AssistantActionKey) => (
    <AssistantModelRow action={action} label={ACTION_LABELS[action]} prefs={assistant.prefs} save={assistant.save} />
  );
  return (
    <UsageEditScope>
      <InspectorBody>
        <Group title="识别">
          <AsrModelRow label="语音输入" hint="说完一段识别一段，准确优先。" />
          <SubtitleAsrRow label="实时字幕" hint="边说边出字，要求速度快。" />
          <TranscriptionModelRow label="录音转写" />
          <OcrModelRow label="屏幕文字" />
        </Group>
        <Group title="文本处理">
          <DefaultLlmRow />
          <SmartLlmRow label="语音输入整理" />
          {assistantRow("editSelection")}
          {assistantRow("ask")}
          <TranslationModelRow label="翻译" hint="字幕翻译和助手的翻译输入共用。可以选专用翻译模型，也可以选大模型。" />
          {assistant.message && <p role="alert" className="px-3.5 py-3 text-xs text-[var(--color-err)]">{assistant.message}</p>}
        </Group>
      </InspectorBody>
    </UsageEditScope>
  );
}
