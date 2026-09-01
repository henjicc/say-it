import type { ProviderProfile } from "@/store/useProviderStore";
import type { ModelPickerOption } from "./ModelPicker";

export const FOLLOW_DEFAULT_LLM_OPTION: ModelPickerOption = {
  value: "default",
  label: "跟随全局默认智能模型",
  providerId: "default",
  providerLabel: "默认设置",
};

export function llmModelsFromProfile(profile: ProviderProfile): string[] {
  const configured = profile.config?.models;
  const models = Array.isArray(configured) ? configured.flatMap((item) => {
    const name = item && typeof item === "object" ? (item as { name?: unknown }).name : undefined;
    return typeof name === "string" && name.trim() ? [name.trim()] : [];
  }) : [];
  const current = profile.config?.model;
  if (typeof current === "string" && current.trim() && !models.includes(current.trim())) {
    models.unshift(current.trim());
  }
  return models;
}

export const llmModelValue = (providerId: string, model: string) => JSON.stringify([providerId, model]);

export function llmModelPickerOptions(profiles: readonly ProviderProfile[]): ModelPickerOption[] {
  return profiles.flatMap((profile) => llmModelsFromProfile(profile).map((model) => ({
    value: llmModelValue(profile.id, model),
    label: model,
    triggerLabel: `${profile.displayName} · ${model}`,
    providerId: profile.id,
    providerLabel: profile.displayName,
  })));
}
