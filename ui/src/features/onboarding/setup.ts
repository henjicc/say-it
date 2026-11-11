import { LLM_PROVIDER_PRESETS } from "@/features/models/llmPresets";
import { gpuEnough, LOCAL_MODEL_NEEDS, type HardwareSummary } from "@/features/models/localModelNeeds";
import { providerReady } from "@/features/models/providerModels";
import { recommendedLlmModels } from "@/features/models/recommendedLlmModels";
import { ASR_API_KEY_URLS, providerApiKeyUrl } from "@/features/settings/apiKeyLinks";
import { useDictPrefs } from "@/store/useDictPrefs";
import {
  useProviderStore,
  type ProviderCapability,
  type ProviderProfile,
} from "@/store/useProviderStore";

/** 引导第一个选择：模型在自己电脑上跑，还是用在线服务。之后各步只列这种方式下的选项。 */
export type UsageMode = "local" | "online";

export const LOCAL_POLISH_PACK_ID = "official.llama-polish-4b";
const LOCAL_POLISH_PROVIDER_ID = "local-polish-4b";
const QWEN_ASR_PACK_ID = "official.transcribe-qwen3-asr-1.7b";
/** 与后端 `smart_text::LOCAL_POLISH_TEMPLATES` 一致。 */
export const LOCAL_POLISH_TEMPLATES = ["context-aware-polish", "polish"];

/** 本地整理是本地方案里最吃显卡的一环，按它判断这台电脑适不适合全本地。 */
export function localPolishFits(hardware: HardwareSummary | undefined): boolean {
  return gpuEnough(hardware, LOCAL_MODEL_NEEDS[LOCAL_POLISH_PACK_ID].gpuMemoryMb!) === true;
}

/** 引导里推荐的识别方式；其余模型在模型页添加。本地的要先下载 `packs` 里的官方模型包。 */
export const ASR_CHOICES = [
  {
    key: "bailian",
    mode: "online",
    label: "阿里云百炼 · Fun-ASR 实时",
    description: "识别准、出字快，同一个密钥也能用于智能整理",
    providerId: "bailian",
    model: "fun-asr-realtime-2026-02-28",
  },
  {
    key: "volcengine",
    mode: "online",
    label: "火山引擎 · SeedASR 2.0",
    description: "需要火山引擎的语音识别密钥",
    providerId: "volcengine",
    model: "seedasr-2.0-realtime",
  },
  {
    key: "local-qwen",
    mode: "local",
    label: "Qwen3-ASR-1.7B",
    description: "识别准、自带标点；有独立显卡时很快，没有也能用",
    model: "local-qwen3-asr-1.7b-sentence-q5",
    packs: [QWEN_ASR_PACK_ID],
  },
  {
    key: "local",
    mode: "local",
    label: "Paraformer 实时",
    description: "对电脑没有要求，边说边出字",
    model: "local-paraformer-online-int8",
    // Paraformer 只出无标点的文字，要同时装标点模型。
    packs: ["official.sherpa-paraformer-online", "official.sherpa-ct-punct-zh-en"],
  },
] as const;

/** 这种方式下默认选哪个识别模型：在线用百炼；本地有合适的显卡用 Qwen3-ASR，否则用对电脑没要求的 Paraformer。 */
export function defaultAsrChoice(mode: UsageMode, hardware: HardwareSummary | undefined): AsrChoiceKey {
  if (mode === "online") return "bailian";
  return gpuEnough(hardware, LOCAL_MODEL_NEEDS[QWEN_ASR_PACK_ID].gpuMemoryMb!) ? "local-qwen" : "local";
}

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

export type SmartChoice = "smart" | "local" | "rules" | "none";

/**
 * 「智能整理」把所选服务设为通用大模型，听写跟随它；「本地整理」让听写用本地整理模型，不动通用大模型；
 * 「只用本地规则」关掉大模型、打开本地规则；「不整理」两者都关。
 */
export async function applySmartChoice(
  choice: SmartChoice,
  llm: { target: LlmTarget; model: string; apiKey: string },
) {
  const patch = useDictPrefs.getState().patch;
  if (choice === "local") {
    // 本地整理模型只按内置润色模板的规则整理；重新运行引导时如果之前换过别的模板，换回来。
    const template = useDictPrefs.getState().prefs.smartTemplateId;
    await patch({
      smartProcessingEnabled: true,
      smartLlmProviderId: LOCAL_POLISH_PROVIDER_ID,
      smartLlmModel: "",
      ...(LOCAL_POLISH_TEMPLATES.includes(template) ? {} : { smartTemplateId: LOCAL_POLISH_TEMPLATES[0] }),
    });
    return;
  }
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
