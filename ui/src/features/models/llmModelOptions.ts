import type {
  LlmModelAvailability,
  LlmModelConfig,
  LlmModelSource,
  LlmReasoningEffort,
  ProviderProfile,
} from "@/store/useProviderStore";
import type { ModelPickerOption } from "./ModelPicker";
import { isModelOffered } from "./providerModels";
import { sdkPricing } from "@/features/asr/modelRegistry";
import { tokenPrice } from "./pricing";
import { adapterOf, isRecommendedLlmModel, llmModelLabel } from "./recommendedLlmModels";

export const FOLLOW_DEFAULT_LLM_OPTION: ModelPickerOption = {
  value: "default",
  label: "跟随通用大模型",
  providerId: "default",
  providerLabel: "通用",
};

export type ThinkingLevel = "zero" | "low" | "high";

export const THINKING_OPTIONS: { key: ThinkingLevel; label: string }[] = [
  { key: "zero", label: "关闭" },
  { key: "low", label: "低" },
  { key: "high", label: "高" },
];

/** 与后端 `normalized_reasoning_effort` 一致：只有关闭、低、高三档。 */
export function thinkingLevel(value: unknown): ThinkingLevel {
  if (value === "low") return "low";
  if (value === "medium" || value === "high") return "high";
  return "zero";
}

const isSource = (value: unknown): value is LlmModelSource => value === "remote" || value === "manual";
const isAvailability = (value: unknown): value is LlmModelAvailability =>
  value === "available" || value === "missing" || value === "unknown";

export function manualLlmModel(name: string): LlmModelConfig {
  return { name, source: "manual", availability: "unknown", reasoningEffort: "zero", temperature: 0.1, maxTokens: null };
}

/** 读出供应商配置里的大模型列表；供应商当前模型不在列表里时补成手动项。 */
export function llmModelsFromProfile(profile: ProviderProfile): LlmModelConfig[] {
  const rawModels = Array.isArray(profile.config?.models) ? profile.config.models : [];
  const models: LlmModelConfig[] = [];
  for (const value of rawModels) {
    if (!value || typeof value !== "object") continue;
    const raw = value as Record<string, unknown>;
    const name = typeof raw.name === "string" ? raw.name.trim() : "";
    if (!name || models.some((model) => model.name === name)) continue;
    models.push({
      name,
      source: isSource(raw.source) ? raw.source : "manual",
      availability: isAvailability(raw.availability) ? raw.availability : "unknown",
      reasoningEffort: thinkingLevel(raw.reasoningEffort),
      temperature: raw.temperature === null ? null : typeof raw.temperature === "number" ? raw.temperature : 0.1,
      maxTokens: typeof raw.maxTokens === "number" ? raw.maxTokens : null,
    });
  }
  const current = String(profile.config?.model ?? "").trim();
  if (current && !models.some((model) => model.name === current)) models.push(manualLlmModel(current));
  return models.sort((left, right) => left.name.localeCompare(right.name));
}

/** 大模型列表可能有上百个，没设过显示开关时只显示手动添加的、推荐的和供应商当前模型。 */
export function llmModelShownByDefault(profile: ProviderProfile, model: LlmModelConfig): boolean {
  return model.source === "manual"
    || isRecommendedLlmModel(profile, model.name)
    || String(profile.config?.model ?? "").trim() === model.name;
}

export const llmModelValue = (providerId: string, model: string) => JSON.stringify([providerId, model]);

function llmPrice(adapter: string, model: string) {
  const pricing = sdkPricing()?.llm[adapter]?.[model];
  return pricing && tokenPrice(pricing);
}

/** 用途下拉框里的大模型：只列已配置且设为显示的；当前选中的即使被隐藏也保留，避免下拉框显示空白。 */
export function llmModelPickerOptions(profiles: readonly ProviderProfile[], selected?: string): ModelPickerOption[] {
  return profiles.flatMap((profile) => llmModelsFromProfile(profile)
    .filter((model) => {
      const value = llmModelValue(profile.id, model.name);
      return value === selected || isModelOffered(profile, model.name, llmModelShownByDefault(profile, model));
    })
    .map((model) => ({
      value: llmModelValue(profile.id, model.name),
      label: llmModelLabel(profile, model.name),
      triggerLabel: `${profile.displayName} · ${llmModelLabel(profile, model.name)}`,
      providerId: profile.id,
      providerLabel: profile.displayName,
      price: llmPrice(adapterOf(profile.kind), model.name),
    })));
}
