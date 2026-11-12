import { createContext, useContext } from "react";
import { optionsForScene, sdkPricing, useModelCatalogRevision, type AsrModelOption } from "@/features/asr/modelRegistry";
import { TRANSLATION_MODEL_OPTIONS, TRANSLATION_PROVIDER_ID } from "@/features/translation/models";
import { useProviderStore, type ProviderCapability } from "@/store/useProviderStore";
import { useUiStore } from "@/store/useUiStore";
import type { ModelPickerOption } from "./ModelPicker";
import { tokenPrice } from "./pricing";
import { isManagedProvider, isModelOffered, providerReady } from "./providerModels";

/**
 * 各功能用哪个模型，在模型页「用途」和各功能页都能直接选，写的是同一份设置。
 * 少数设置只在「用途」页里才直接写回存储（对应功能页没打开时）。
 */
const UsageEditContext = createContext(false);

export function UsageEditScope({ children }: { children: React.ReactNode }) {
  return <UsageEditContext.Provider value>{children}</UsageEditContext.Provider>;
}

/** 是否渲染在「用途」页里。 */
export function useInUsagePage(): boolean {
  return useContext(UsageEditContext);
}

/** 场景下拉框里的识别模型：按供应商密钥与显示开关过滤，当前选中的始终保留。 */
export function useOfferedAsrOptions(options: readonly AsrModelOption[], selected: string): AsrModelOption[] {
  useModelCatalogRevision();
  const profiles = useProviderStore((state) => state.profiles);
  return options.filter((option) => option.value === selected
    || isModelOffered(profiles.find((profile) => profile.id === option.providerId), option.value, true, "asr"));
}

/**
 * 内置 Qwen-MT 的 SDK 能力模块 ID 为 `bailian.translation.<模型>`（SDK `BAILIAN_QWEN_MT_PRESETS` 的 moduleId），
 * 与运行时注册的翻译模块一致。
 */
export function translationPrice(model: string) {
  const pricing = sdkPricing()?.capabilities[`bailian.translation.${model}`];
  return pricing?.billing === "tokens" ? tokenPrice(pricing) : undefined;
}

/** 专用翻译模型：内置 Qwen-MT 加插件提供的翻译模型，按供应商密钥与显示开关过滤，当前选中的始终保留。 */
export function useOfferedTranslationOptions(selected: string): Array<ModelPickerOption & { providerId: string }> {
  useModelCatalogRevision();
  const profiles = useProviderStore((state) => state.profiles);
  const providerLabel = profiles.find((profile) => profile.id === TRANSLATION_PROVIDER_ID)?.displayName;
  return [
    ...TRANSLATION_MODEL_OPTIONS.map(({ value, label }) => ({
      value,
      label,
      providerId: TRANSLATION_PROVIDER_ID,
      providerLabel,
      price: translationPrice(value),
    })),
    ...optionsForScene("subtitleTranslation"),
  ].filter((option) => option.value === selected
    || isModelOffered(profiles.find((profile) => profile.id === option.providerId), option.value, true, "translation"));
}

/** 当前选择不能用时，返回写在用途行下方的原因和处理入口；能用时返回 null。 */
export function useUsageIssue(
  providerId: string | undefined,
  modelKnown = true,
  capability?: ProviderCapability,
): React.ReactNode {
  const profile = useProviderStore((state) => state.profiles.find((item) => item.id === providerId));
  if (!modelKnown) return <span role="status" className="text-[var(--color-err)]">这个模型已下线，请重新选择。</span>;
  if (!profile || !isManagedProvider(profile) || providerReady(profile, capability)) return null;
  const missingKey = profile.configFields?.find((field) => field.secret && field.capability === capability)?.label ?? "密钥";
  return (
    <span role="status" className="text-[var(--color-err)]">
      {profile.displayName} 还没有填写{missingKey}。
      <button type="button" className="ml-1 text-[var(--color-accent-text)] hover:underline" onClick={() => useUiStore.getState().openModels(`provider:${profile.id}`)}>
        去填写
      </button>
    </span>
  );
}

/** 识别模型的用途提示：插件模型不在内置目录里，按场景选项判断是否仍然存在。 */
export function useAsrUsageIssue(model: string, scenes: readonly string[]): React.ReactNode {
  useModelCatalogRevision();
  const option = scenes.flatMap((scene) => optionsForScene(scene)).find((item) => item.value === model);
  return useUsageIssue(option?.providerId, Boolean(option), "asr");
}
