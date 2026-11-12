import { useState } from "react";
import { Row, ROW_CONTROL_W } from "@/components/ui/Group";
import { ModelPicker, type ModelPickerOption } from "@/features/models/ModelPicker";
import { FOLLOW_DEFAULT_LLM_OPTION, llmModelPickerOptions, llmModelValue } from "@/features/models/llmModelOptions";
import { useOfferedTranslationOptions, useUsageIssue } from "@/features/models/usage";
import { useProviderStore } from "@/store/useProviderStore";
import { useTranslationStore, type TranslationPrefs } from "@/store/useTranslationStore";

/**
 * 下拉框的取值：大模型沿用 `llmModelValue` 的 JSON 数组和「default」，
 * 专用翻译模型直接用模型 ID（不会以「[」开头，也不会叫 default）。
 */
function choiceValue(prefs: TranslationPrefs): string {
  if (prefs.engine === "dedicated") return prefs.model;
  return prefs.llmProviderId === "default" ? "default" : llmModelValue(prefs.llmProviderId, prefs.llmModel);
}

function prefsFromChoice(value: string): TranslationPrefs {
  if (value === "default") return { engine: "llm", model: "", llmProviderId: "default", llmModel: "" };
  if (value.startsWith("[")) {
    const [llmProviderId, llmModel] = JSON.parse(value) as [string, string];
    return { engine: "llm", model: "", llmProviderId, llmModel };
  }
  return { engine: "dedicated", model: value, llmProviderId: "default", llmModel: "" };
}

/** 共用的翻译模型：一个下拉框里同时列专用翻译模型和大模型，字幕翻译和助手的翻译输入都用它。 */
export function TranslationModelRow({ label = "翻译模型", hint }: { label?: string; hint?: string }) {
  const prefs = useTranslationStore((state) => state.prefs);
  const save = useTranslationStore((state) => state.save);
  const profiles = useProviderStore((state) => state.profiles).filter((profile) => profile.enabled && profile.capabilities.includes("llm"));
  const [error, setError] = useState("");
  const value = choiceValue(prefs);
  const dedicated = useOfferedTranslationOptions(prefs.engine === "dedicated" ? prefs.model : "");
  const options: ModelPickerOption[] = [
    ...dedicated.map((option) => ({ ...option, providerLabel: `专用翻译模型 · ${option.providerLabel ?? ""}`.replace(/ · $/, "") })),
    FOLLOW_DEFAULT_LLM_OPTION,
    ...llmModelPickerOptions(profiles, value),
  ];
  const selected = options.find((option) => option.value === value);
  const issue = useUsageIssue(
    prefs.engine === "dedicated" ? selected?.providerId : prefs.llmProviderId === "default" ? undefined : prefs.llmProviderId,
    Boolean(selected),
    prefs.engine === "dedicated" ? "translation" : "llm",
  );
  const message = error ? <span role="alert" className="text-[var(--color-err)]">{error}</span> : issue;
  return (
    <Row label={label} hint={hint} message={message}>
      <div className={ROW_CONTROL_W}>
        <ModelPicker
          aria-label={label}
          value={value}
          options={options}
          panelLabel="选择翻译模型"
          onChange={(next) => {
            setError("");
            save(prefsFromChoice(next)).catch((reason) => setError(`保存失败：${String(reason)}`));
          }}
        />
      </div>
    </Row>
  );
}
