import { useState } from "react";
import { Row } from "@/components/ui/Group";
import { Tabs } from "@/components/ui/Tabs";
import { useProviderStore, type LlmReasoningEffort } from "@/store/useProviderStore";
import { THINKING_OPTIONS, llmModelsFromProfile, thinkingLevel } from "./llmModelOptions";
import { useProviderConfigQueue } from "./providerConfigQueue";

/**
 * 「用途」页里紧跟在大模型选择下方的思考程度。设置存在模型上，用同一个模型的功能共用。
 */
export function LlmThinkingRow({ providerId, model }: { providerId: string; model: string }) {
  const profile = useProviderStore((state) => state.profiles.find((item) => item.id === providerId));
  const queue = useProviderConfigQueue(providerId);
  const [error, setError] = useState("");
  const config = profile && llmModelsFromProfile(profile).find((item) => item.name === model);
  if (!profile || !config) return null;

  const change = (reasoningEffort: LlmReasoningEffort) => {
    setError("");
    queue((latest) => ({
      models: llmModelsFromProfile(latest).map((item) => item.name === model ? { ...item, reasoningEffort } : item),
    })).catch((reason) => setError(`保存失败：${String(reason)}`));
  };

  return (
    <Row
      label="思考程度"
      hint="开启后回答更周全，但更慢、费用更高。不能关闭思考的模型选「关闭」时按最低档。同一个模型在各功能里共用这个设置。"
      message={error && <span role="alert" className="text-[var(--color-err)]">{error}</span>}
    >
      <Tabs variant="view" ariaLabel={`${model} 思考程度`} tabs={THINKING_OPTIONS} active={thinkingLevel(config.reasoningEffort)} onChange={change} />
    </Row>
  );
}
