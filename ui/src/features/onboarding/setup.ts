import { LLM_PROVIDER_PRESETS } from "@/features/models/llmPresets";
import { gpuEnough, LOCAL_MODEL_NEEDS, type HardwareSummary } from "@/features/models/localModelNeeds";
import { providerReady, providerSecretFields } from "@/features/models/providerModels";
import { recommendedLlmModels } from "@/features/models/recommendedLlmModels";
import { ASR_API_KEY_URLS, providerApiKeyUrl } from "@/features/settings/apiKeyLinks";
import { useDictPrefs } from "@/store/useDictPrefs";
import {
  useProviderStore,
  type ProviderCapability,
  type ProviderProfile,
} from "@/store/useProviderStore";

/** 引导使用方式：离线模式（本地）或在自己账号下接入第三方在线服务。 */
export type UsageMode = "local" | "online";
export type LocalPlanKey = "lite" | "full";
export type LocalAsrTier = "0.6b" | "1.7b";
export type OnlineBundleId = "bailian" | "volcengine" | "siliconflow" | "llm-groq";

/** 本地润色模型包（单一常量，后续 1.7B 润色包上线时只需改此处与对应 provider ID）。 */
export const LOCAL_POLISH_PACK_ID = "official.llama-polish-4b";
export const LOCAL_POLISH_PROVIDER_ID = "local-polish-4b";
/** 与后端 `smart_text::LOCAL_POLISH_TEMPLATES` 一致。 */
export const LOCAL_POLISH_TEMPLATES = ["context-aware-polish", "polish"];

export const CUSTOM_DATA_SUBDIR = "SayItData";

export const LOCAL_LITE_ASR_MODEL = "local-funasr-nano-sentence-int8";
export const LOCAL_LITE_PACKS = [
  { id: "official.sherpa-funasr-nano-ctc", name: "Fun-ASR-Nano 语音识别", fallbackSize: "约 550 MB" },
  { id: "official.sherpa-ct-punct-zh-en", name: "中英自动标点", fallbackSize: "约 100 MB" },
] as const;

export const LOCAL_FULL_ASR_TIERS = {
  "0.6b": {
    id: "official.transcribe-qwen3-asr-0.6b",
    name: "Qwen3-ASR-0.6B 听写",
    label: "0.6B 轻快",
    model: "local-qwen3-asr-0.6b-sentence-q5",
    fallbackSize: "约 620 MB",
  },
  "1.7b": {
    id: "official.transcribe-qwen3-asr-1.7b",
    name: "Qwen3-ASR-1.7B 听写",
    label: "1.7B 高准",
    model: "local-qwen3-asr-1.7b-sentence-q5",
    fallbackSize: "约 1.8 GB",
  },
} as const;

export const LOCAL_POLISH_PACK_INFO = {
  id: LOCAL_POLISH_PACK_ID,
  name: "本地智能润色模型",
  fallbackSize: "约 3.5 GB",
} as const;

export interface LocalPackItem {
  id: string;
  name: string;
  fallbackSize: string;
}

export function localPlanPacks(plan: LocalPlanKey, asrTier: LocalAsrTier): LocalPackItem[] {
  if (plan === "lite") return [...LOCAL_LITE_PACKS];
  const asr = LOCAL_FULL_ASR_TIERS[asrTier];
  return [
    { id: asr.id, name: asr.name, fallbackSize: asr.fallbackSize },
    LOCAL_POLISH_PACK_INFO,
  ];
}

/** 本地润色是本地方案里最吃显卡的一环，按它判断这台电脑是否适合全能本地方案。 */
export function localPolishFits(hardware: HardwareSummary | undefined): boolean {
  return gpuEnough(hardware, LOCAL_MODEL_NEEDS[LOCAL_POLISH_PACK_ID].gpuMemoryMb!) === true;
}

/** 硬件检测已返回且明确不满足本地润色显存门槛（避免尚未检测到硬件时误禁用）。 */
export function localPolishDisallowed(hardware: HardwareSummary | undefined): boolean {
  return gpuEnough(hardware, LOCAL_MODEL_NEEDS[LOCAL_POLISH_PACK_ID].gpuMemoryMb!) === false;
}

export function localPolishVramHint(hardware: HardwareSummary | undefined): string {
  const minGb = Math.round((LOCAL_MODEL_NEEDS[LOCAL_POLISH_PACK_ID].gpuMemoryMb ?? 6144) / 1024);
  if (hardware?.unifiedMemory) {
    return `当前 Mac 可用显存偏小（建议至少 ${minGb} GB 统一内存余量），不推荐本地运行润色大模型；如需 AI 润色建议改用在线服务。`;
  }
  return `未检测到满足要求的独立显卡（建议至少 ${minGb} GB 显存），不推荐本地运行润色大模型；如需 AI 润色建议改用在线服务。`;
}

/**
 * 与后端 `data_root::resolve_custom_target_dir` 一致的目录预览：
 * - 选默认目录本身时不套子文件夹；
 * - 路径末尾已是 `SayItData`（忽略大小写）时不重复嵌套；
 * - 其余目录（含 `D:\`、`E:\`、`/` 等磁盘根目录）自动追加 `SayItData`。
 */
export function resolveDataDirPreview(
  selected: string,
  defaultRoot: string,
  isMac = false,
): string {
  const trimmed = selected.trim();
  const normalizedDefault = defaultRoot.trim().replace(/[\\/]+$/, "");
  if (!trimmed) return defaultRoot;

  // 根路径（如 D:\ 或 /）去掉末尾斜杠后只剩盘符或空串，特殊处理。
  const isWinDriveRoot = /^[a-zA-Z]:[\\/]+$/.test(trimmed);
  const isUnixRoot = /^\/+$/.test(trimmed);
  if (isWinDriveRoot) {
    return `${trimmed.slice(0, 2)}\\${CUSTOM_DATA_SUBDIR}`;
  }
  if (isUnixRoot) {
    return `/${CUSTOM_DATA_SUBDIR}`;
  }

  const clean = trimmed.replace(/[\\/]+$/, "");
  if (normalizedDefault && clean.toLowerCase() === normalizedDefault.toLowerCase()) {
    return defaultRoot;
  }
  if (new RegExp(`[\\\\/]${CUSTOM_DATA_SUBDIR}$`, "i").test(clean)) {
    return clean;
  }
  const sep = isMac || (clean.includes("/") && !clean.includes("\\")) ? "/" : "\\";
  return `${clean}${sep}${CUSTOM_DATA_SUBDIR}`;
}

export interface OnlineBundleKeySpec {
  capability: ProviderCapability;
  field: string;
  label: string;
  placeholder: string;
}

export interface OnlineBundle {
  id: OnlineBundleId;
  providerId: string;
  kind: string;
  name: string;
  tag: string;
  accent?: boolean;
  line: string;
  note: string;
  asrModel: string;
  keys: readonly OnlineBundleKeySpec[];
}

/** 引导里内置的 4 个「语音识别 + 大模型润色」一站式服务商。 */
export const ONLINE_BUNDLES: readonly OnlineBundle[] = [
  {
    id: "bailian",
    providerId: "bailian",
    kind: "sdk:bailian",
    name: "阿里云百炼",
    tag: "推荐 · 1 把密钥",
    accent: true,
    line: "实时边说边出字 · 识别与润色一次搞定",
    note: "国内直连稳定，配置最省心",
    asrModel: "fun-asr-realtime-2026-02-28",
    keys: [
      { capability: "asr", field: "apiKey", label: "百炼 API Key", placeholder: "sk-..." },
    ],
  },
  {
    id: "volcengine",
    providerId: "volcengine",
    kind: "sdk:volcengine",
    name: "火山引擎",
    tag: "中文极准 · 2 把密钥",
    line: "豆包实时语音 + 方舟大模型润色",
    note: "需分别填写语音和方舟两把密钥",
    asrModel: "seedasr-2.0-realtime",
    keys: [
      { capability: "asr", field: "apiKey", label: "语音服务密钥", placeholder: "粘贴语音 API Key" },
      { capability: "llm", field: "arkApiKey", label: "方舟大模型 API Key", placeholder: "粘贴方舟 API Key" },
    ],
  },
  {
    id: "siliconflow",
    providerId: "siliconflow",
    kind: "sdk:siliconflow",
    name: "硅基流动 · DeepSeek",
    tag: "1 把密钥",
    line: "SenseVoice 整句识别 + DeepSeek 润色",
    note: "说完松手后整句出字（非实时）",
    asrModel: "FunAudioLLM/SenseVoiceSmall",
    keys: [
      { capability: "asr", field: "apiKey", label: "硅基流动 API Key", placeholder: "sk-..." },
    ],
  },
  {
    id: "llm-groq",
    providerId: "llm-groq",
    kind: "llm:groq",
    name: "Groq",
    tag: "免费额度",
    line: "Whisper 整句识别 + 极速大模型",
    note: "有免费额度，但较易限流且需海外网络",
    asrModel: "whisper-large-v3-turbo",
    keys: [
      { capability: "asr", field: "apiKey", label: "Groq API Key", placeholder: "gsk_..." },
    ],
  },
];

function secretField(profile: ProviderProfile, capability: ProviderCapability) {
  return providerSecretFields(profile, capability)[0];
}

function secretFieldFor(profile: ProviderProfile, capability: ProviderCapability): string {
  return secretField(profile, capability)?.key ?? "apiKey";
}

export function apiKeyUrlForBundle(
  bundle: OnlineBundle,
  keySpec: OnlineBundleKeySpec,
  profile?: ProviderProfile,
): string | undefined {
  const index = keySpec.field === "apiKey" ? 0 : 1;
  if (profile) {
    return keySpec.field === "apiKey"
      ? ASR_API_KEY_URLS[profile.id] ?? providerApiKeyUrl(profile, keySpec.field, 0)
      : providerApiKeyUrl(profile, keySpec.field, index);
  }
  return keySpec.field === "apiKey"
    ? ASR_API_KEY_URLS[bundle.providerId]
    : providerApiKeyUrl({ id: bundle.providerId, kind: bundle.kind }, keySpec.field, index);
}

export function bundleAlreadyConfigured(bundle: OnlineBundle, profiles: ProviderProfile[]): boolean {
  const profile = profiles.find((item) => item.id === bundle.providerId);
  if (!profile) return false;
  return bundle.keys.every((spec) => providerReady(profile, spec.capability));
}

export function deepseekPreset() {
  const preset = LLM_PROVIDER_PRESETS.find((item) => item.adapter === "deepseek");
  if (!preset) throw new Error("没有 deepseek 的大模型预设");
  return preset;
}

export function deepseekProfile(profiles: ProviderProfile[]): ProviderProfile | undefined {
  return profiles.find((item) => item.kind === "llm:deepseek");
}

export function deepseekAlreadyConfigured(profiles: ProviderProfile[]): boolean {
  const profile = deepseekProfile(profiles);
  return Boolean(profile && providerReady(profile, "llm"));
}

function defaultLlmModelForKind(kind: string, profile?: ProviderProfile): string {
  const existing = String(profile?.config?.model ?? "").trim();
  if (existing) return existing;
  return recommendedLlmModels(kind)[0]?.id ?? "";
}

/** 一键应用本地方案：同时写入听写识别模型与智能整理/本地规则开关。 */
export async function applyLocalPlan(plan: LocalPlanKey, asrTier: LocalAsrTier) {
  const patch = useDictPrefs.getState().patch;
  if (plan === "lite") {
    await patch({
      asrModel: LOCAL_LITE_ASR_MODEL,
      smartProcessingEnabled: false,
      localRulesEnabled: true,
    });
    return;
  }
  const asr = LOCAL_FULL_ASR_TIERS[asrTier];
  const template = useDictPrefs.getState().prefs.smartTemplateId;
  await patch({
    asrModel: asr.model,
    smartProcessingEnabled: true,
    smartLlmProviderId: LOCAL_POLISH_PROVIDER_ID,
    smartLlmModel: "",
    ...(LOCAL_POLISH_TEMPLATES.includes(template) ? {} : { smartTemplateId: LOCAL_POLISH_TEMPLATES[0] }),
  });
}

/** 一键配齐在线一站式服务商：保存 1～2 把密钥、设置默认听写识别模型、配置默认润色大模型并开启智能整理。 */
export async function applyOnlineBundle(
  bundle: OnlineBundle,
  draftKeys: Record<string, string>,
) {
  const store = useProviderStore.getState();
  const profile = store.profiles.find((item) => item.id === bundle.providerId);
  if (!profile) throw new Error(`没有找到 ${bundle.name} 供应商`);

  const configPatch: Record<string, unknown> = {};
  for (const spec of bundle.keys) {
    const value = (draftKeys[spec.field] ?? "").trim();
    if (value) {
      const targetField = secretFieldFor(profile, spec.capability) || spec.field;
      configPatch[targetField] = value;
    }
  }
  const model = defaultLlmModelForKind(profile.kind, profile);
  if (model) configPatch.model = model;

  if (Object.keys(configPatch).length > 0) {
    await store.updateConfig(profile.id, configPatch);
  }
  if (useProviderStore.getState().defaults.llm !== profile.id) {
    await store.setDefault("llm", profile.id);
  }
  await useDictPrefs.getState().patch({
    asrModel: bundle.asrModel,
    smartProcessingEnabled: true,
    smartLlmProviderId: "default",
    smartLlmModel: "",
  });
}

/** 折叠混搭方案：本地免费 Fun-ASR-Nano 识别 + DeepSeek 官方 API 润色。 */
export async function applyDeepSeekMix(apiKey: string) {
  const store = useProviderStore.getState();
  const preset = deepseekPreset();
  let profile = deepseekProfile(store.profiles);
  const model = defaultLlmModelForKind("llm:deepseek", profile);
  const trimmedKey = apiKey.trim();

  if (!profile) {
    profile = await store.addLlmProvider({
      adapter: preset.adapter,
      displayName: preset.name,
      model,
      apiKey: trimmedKey,
      endpoint: "",
    });
  } else {
    const patch: Record<string, unknown> = {};
    if (trimmedKey) patch.apiKey = trimmedKey;
    if (model) patch.model = model;
    if (Object.keys(patch).length > 0) {
      await store.updateConfig(profile.id, patch);
    }
  }
  if (useProviderStore.getState().defaults.llm !== profile.id) {
    await store.setDefault("llm", profile.id);
  }
  await useDictPrefs.getState().patch({
    asrModel: LOCAL_LITE_ASR_MODEL,
    smartProcessingEnabled: true,
    smartLlmProviderId: "default",
    smartLlmModel: "",
  });
}
