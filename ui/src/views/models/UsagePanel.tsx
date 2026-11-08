import { useState } from "react";
import { InspectorBody } from "@/components/layout/MasterDetail";
import { Group, Row, ROW_CONTROL_W } from "@/components/ui/Group";
import { LlmThinkingRow } from "@/features/models/LlmThinkingRow";
import { ModelPicker } from "@/features/models/ModelPicker";
import { llmModelPickerOptions, llmModelValue } from "@/features/models/llmModelOptions";
import { UsageEditScope, useUsageIssue } from "@/features/models/usage";
import { useProviderStore } from "@/store/useProviderStore";
import { AsrModelRow } from "@/views/dictation/DictationSections";
import { OcrModelRow, SmartLlmRow } from "@/views/smart-text/ModelRows";
import { SubtitleAsrRow } from "@/views/subtitles/SubtitleSections";
import { SubtitleTranslationModelRow } from "@/views/SubtitleTranslationPanel";
import { TranscriptionModelRow } from "@/views/TranscriptionView";
import { AssistantModelRows } from "@/views/VoiceAssistantPanel";

/** 通用大模型：没单独选模型的功能（含「跟随通用大模型」与自动学习）使用它。 */
function GeneralLlmRow() {
  const profiles = useProviderStore((state) => state.profiles).filter(
    (profile) => profile.enabled && profile.capabilities.includes("llm"),
  );
  const defaults = useProviderStore((state) => state.defaults);
  const setDefault = useProviderStore((state) => state.setDefault);
  const updateConfig = useProviderStore((state) => state.updateConfig);
  const [error, setError] = useState("");
  const current = profiles.find((profile) => profile.id === defaults.llm);
  const selected = current ? llmModelValue(current.id, String(current.config?.model ?? "")) : "";
  const issue = useUsageIssue(current?.id);

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
        label="通用大模型"
        controlId="usage-general-llm"
        hint="没有单独选择模型的功能会使用它，例如选了「跟随通用大模型」的智能处理和助手。"
        message={error ? <span role="alert" className="text-[var(--color-err)]">{error}</span> : issue}
      >
        <div className={ROW_CONTROL_W}>
          <ModelPicker
            id="usage-general-llm"
            value={selected}
            options={llmModelPickerOptions(profiles, selected)}
            panelLabel="选择通用大模型"
            onChange={(value) => void change(value)}
          />
        </div>
      </Row>
      {current && <LlmThinkingRow providerId={current.id} model={String(current.config?.model ?? "")} />}
    </>
  );
}

/** 各功能用哪个模型，只在这里设置；下拉框里只列已填密钥、并在供应商里设为显示的模型。 */
export function UsagePanel() {
  return (
    <UsageEditScope>
      <InspectorBody>
        <Group title="通用"><GeneralLlmRow /></Group>
        <Group title="听写"><AsrModelRow /><SmartLlmRow /></Group>
        <Group title="字幕"><SubtitleAsrRow /><SubtitleTranslationModelRow /></Group>
        <Group title="转写"><TranscriptionModelRow /></Group>
        <Group title="助手"><AssistantModelRows /></Group>
        <Group title="读屏"><OcrModelRow /></Group>
      </InspectorBody>
    </UsageEditScope>
  );
}
