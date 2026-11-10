import { LLM_PROVIDER_PRESETS } from "@/features/models/llmPresets";
import { providerReady } from "@/features/models/providerModels";
import { recommendedLlmModels } from "@/features/models/recommendedLlmModels";
import { ASR_API_KEY_URLS, providerApiKeyUrl } from "@/features/settings/apiKeyLinks";
import { useDictPrefs } from "@/store/useDictPrefs";
import {
  useProviderStore,
  type ProviderCapability,
  type ProviderProfile,
} from "@/store/useProviderStore";

/** 引导里推荐的三种识别方式；其余模型在模型页添加。 */
export const ASR_CHOICES = [
  {
    key: "bailian",
    label: "阿里云百炼 · Fun-ASR 实时",
    description: "识别准、出字快，同一个密钥也能用于智能整理",
    providerId: "bailian",
    model: "fun-asr-realtime-2026-02-28",
    recommended: true,
  },
  {
    key: "volcengine",
    label: "火山引擎 · SeedASR 2.0",
    description: "需要火山引擎的语音识别密钥",
    providerId: "volcengine",
    model: "seedasr-2.0-realtime",
  },
  {
    key: "local",
    label: "本地离线 · Paraformer 实时",
    description: "完全免费、断网可用",
    model: "local-paraformer-online-int8",
  },
] as const;

export type AsrChoice = (typeof ASR_CHOICES)[number];
export type AsrChoiceKey = AsrChoice["key"];

/** 有专管这种能力的密钥（如火山引擎的方舟密钥只管大模型）时用它，否则用通用的 apiKey。 */
function scopedSecretField(profile: ProviderProfile, capability: ProviderCapability) {
  return (profile.configFields ?? []).find((field) => field.secret && field.capability === capability);
}

function secretFieldFor(profile: ProviderProfile, capability: ProviderCapability): string {
  return scopedSecretField(profile, capability)?.key ?? "apiKey";
}

/** 密钥输入框的名称：专用密钥用它自己的字段名（如「方舟 API Key」），避免把另一把密钥贴进来。 */
export function apiKeyLabelFor(profile: ProviderProfile, capability: ProviderCapability): string {
  return scopedSecretField(profile, capability)?.label ?? `${profile.displayName} API Key`;
}

export function apiKeyUrlFor(profile: ProviderProfile, capability: ProviderCapability): string | undefined {
  const field = secretFieldFor(profile, capability);
  return field === "apiKey" ? ASR_API_KEY_URLS[profile.id] ?? providerApiKeyUrl(profile, field, 0) : providerApiKeyUrl(profile, field, 1);
}

export async function saveProviderKey(profile: ProviderProfile, capability: ProviderCapability, key: string) {
  await useProviderStore.getState().updateConfig(profile.id, { [secretFieldFor(profile, capability)]: key });
}

/** 智能整理可选的大模型服务：内置的直接用现成供应商，其余按预设添加。 */
export const LLM_TARGETS = [
  { key: "bailian", providerId: "bailian" },
  { key: "deepseek", adapter: "deepseek" },
  { key: "volcengine", providerId: "volcengine" },
  { key: "kimi", adapter: "kimi" },
  { key: "bigmodel", adapter: "bigmodel" },
  { key: "siliconflow", providerId: "siliconflow" },
] as const;

export type LlmTarget = (typeof LLM_TARGETS)[number];
export type LlmTargetKey = LlmTarget["key"];

function presetOf(adapter: string) {
  const preset = LLM_PROVIDER_PRESETS.find((item) => item.adapter === adapter);
  if (!preset) throw new Error(`没有 ${adapter} 的大模型预设`);
  return preset;
}

/** 预设类服务只看已添加的同类供应商；内置服务按 ID 找。 */
export function llmProfileOf(target: LlmTarget, profiles: ProviderProfile[]): ProviderProfile | undefined {
  return "providerId" in target
    ? profiles.find((profile) => profile.id === target.providerId)
    : profiles.find((profile) => profile.kind === `llm:${target.adapter}`);
}

export function llmTargetLabel(target: LlmTarget, profiles: ProviderProfile[]): string {
  const profile = llmProfileOf(target, profiles);
  if (profile) return profile.displayName;
  return "adapter" in target ? presetOf(target.adapter).name : target.key;
}

/** 推荐模型；已经配置过别的模型（例如重新运行引导）时保留并排在最前，默认选中它。 */
export function llmTargetModels(target: LlmTarget, profiles: ProviderProfile[]) {
  const recommended = recommendedLlmModels("adapter" in target ? `llm:${target.adapter}` : `sdk:${target.providerId}`);
  const current = String(llmProfileOf(target, profiles)?.config?.model ?? "");
  return current && !recommended.some((model) => model.id === current)
    ? [{ id: current, label: current }, ...recommended]
    : recommended;
}

export function llmTargetDefaultModel(target: LlmTarget, profiles: ProviderProfile[]): string {
  const current = String(llmProfileOf(target, profiles)?.config?.model ?? "");
  return current || llmTargetModels(target, profiles)[0].id;
}

export function llmTargetKeyLabel(target: LlmTarget, profiles: ProviderProfile[]): string {
  const profile = llmProfileOf(target, profiles);
  return profile ? apiKeyLabelFor(profile, "llm") : `${llmTargetLabel(target, profiles)} API Key`;
}

export function llmTargetKeyUrl(target: LlmTarget, profiles: ProviderProfile[]): string | undefined {
  if ("adapter" in target) return presetOf(target.adapter).apiKeyUrl ?? undefined;
  const profile = llmProfileOf(target, profiles);
  return profile && apiKeyUrlFor(profile, "llm");
}

export function llmTargetReady(target: LlmTarget, profiles: ProviderProfile[]): boolean {
  const profile = llmProfileOf(target, profiles);
  return Boolean(profile && providerReady(profile, "llm"));
}

export type SmartChoice = "smart" | "rules" | "none";

/**
 * 「智能整理」把所选服务设为通用大模型，听写跟随它；「只用本地规则」关掉大模型、打开本地规则；
 * 「不整理」两者都关。
 */
export async function applySmartChoice(
  choice: SmartChoice,
  llm: { target: LlmTarget; model: string; apiKey: string },
) {
  const patch = useDictPrefs.getState().patch;
  if (choice !== "smart") {
    await patch({ smartProcessingEnabled: false, localRulesEnabled: choice === "rules" });
    return;
  }
  const store = useProviderStore.getState();
  const { target, model, apiKey } = llm;
  let profile = llmProfileOf(target, store.profiles);
  if (!profile && "adapter" in target) {
    profile = await store.addLlmProvider({ adapter: target.adapter, displayName: presetOf(target.adapter).name, model, apiKey, endpoint: "" });
  } else if (!profile) {
    throw new Error(`没有找到 ${target.key} 供应商`);
  } else {
    if (apiKey) await saveProviderKey(profile, "llm", apiKey);
    await store.updateConfig(profile.id, { model });
  }
  if (useProviderStore.getState().defaults.llm !== profile.id) await store.setDefault("llm", profile.id);
  await patch({ smartProcessingEnabled: true, smartLlmProviderId: "default", smartLlmModel: "" });
}
