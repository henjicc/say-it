import catalog from "~shared/llm-recommended-models.json";
import type { ProviderProfile } from "@/store/useProviderStore";

type RecommendedModel = { id: string; label: string };

const adapters: Record<string, RecommendedModel[] | undefined> = catalog.adapters;

/** 与后端 `llm_adapter` 一致：内置百炼走 bailian，其余取 `llm:` 之后的部分。 */
function adapterOf(kind: string): string {
  return kind === "sdk:bailian" ? "bailian" : kind.replace(/^llm:/, "");
}

/** 供应商的推荐模型；后端按同一份目录把它们补进模型列表。 */
export function recommendedLlmModels(kind: string): RecommendedModel[] {
  return adapters[adapterOf(kind)] ?? [];
}

export function isRecommendedLlmModel(profile: ProviderProfile, name: string): boolean {
  return recommendedLlmModels(profile.kind).some((model) => model.id === name);
}

/** 推荐模型用规范的显示名，其余直接显示模型 ID。 */
export function llmModelLabel(profile: ProviderProfile, name: string): string {
  return recommendedLlmModels(profile.kind).find((model) => model.id === name)?.label ?? name;
}
