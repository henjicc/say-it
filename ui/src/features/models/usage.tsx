import { createContext, useContext } from "react";
import { ArrowRight } from "lucide-react";
import { Row } from "@/components/ui/Group";
import { optionsForScene, useModelCatalogRevision, type AsrModelOption } from "@/features/asr/modelRegistry";
import { TRANSLATION_MODEL_OPTIONS, TRANSLATION_PROVIDER_ID } from "@/features/translation/models";
import { useProviderStore } from "@/store/useProviderStore";
import { useUiStore } from "@/store/useUiStore";
import { isManagedProvider, isModelOffered, providerReady } from "./providerModels";

/**
 * 各功能用哪个模型只在模型页「用途」里设置。功能页的同一行在专业模式下只显示当前值并跳转过去；
 * 简单模式没有模型页，仍在功能页直接选择。
 */
const UsageEditContext = createContext(false);

export function UsageEditScope({ children }: { children: React.ReactNode }) {
  return <UsageEditContext.Provider value>{children}</UsageEditContext.Provider>;
}

/** 是否渲染在「用途」页里；功能页没打开时，个别设置要直接写回存储。 */
export function useInUsagePage(): boolean {
  return useContext(UsageEditContext);
}

export function useUsageEditable(): boolean {
  const inUsagePage = useInUsagePage();
  const simple = useUiStore((state) => state.layoutMode === "simple");
  return inUsagePage || simple;
}

export function UsageLinkRow({ label, value, message }: { label: string; value: string; message?: React.ReactNode }) {
  return (
    <Row label={label} message={message}>
      <button
        type="button"
        onClick={() => useUiStore.getState().openModels("usage")}
        className="flex max-w-64 items-center gap-1.5 rounded-[var(--radius-sm)] px-2 py-1 text-[13px] text-[var(--color-fg-muted)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-fg)]"
      >
        <span className="truncate">{value}</span>
        <ArrowRight className="h-3.5 w-3.5 flex-none" aria-hidden />
      </button>
    </Row>
  );
}

/** 场景下拉框里的识别模型：按供应商密钥与显示开关过滤，当前选中的始终保留。 */
export function useOfferedAsrOptions(options: readonly AsrModelOption[], selected: string): AsrModelOption[] {
  useModelCatalogRevision();
  const profiles = useProviderStore((state) => state.profiles);
  return options.filter((option) => option.value === selected
    || isModelOffered(profiles.find((profile) => profile.id === option.providerId), option.value, true));
}

/** 专用翻译模型：内置 Qwen-MT 加插件提供的翻译模型，按供应商密钥与显示开关过滤，当前选中的始终保留。 */
export function useOfferedTranslationOptions(selected: string): { value: string; label: string; providerId: string }[] {
  useModelCatalogRevision();
  const profiles = useProviderStore((state) => state.profiles);
  return [
    ...TRANSLATION_MODEL_OPTIONS.map((option) => ({ ...option, providerId: TRANSLATION_PROVIDER_ID })),
    ...optionsForScene("subtitleTranslation"),
  ].filter((option) => option.value === selected
    || isModelOffered(profiles.find((profile) => profile.id === option.providerId), option.value, true));
}

/** 当前选择不能用时，返回写在用途行下方的原因和处理入口；能用时返回 null。 */
export function useUsageIssue(providerId: string | undefined, modelKnown = true): React.ReactNode {
  const profile = useProviderStore((state) => state.profiles.find((item) => item.id === providerId));
  if (!modelKnown) return <span role="status" className="text-[var(--color-err)]">这个模型已下线，请重新选择。</span>;
  if (!profile || !isManagedProvider(profile) || providerReady(profile)) return null;
  return (
    <span role="status" className="text-[var(--color-err)]">
      {profile.displayName} 还没有填写密钥。
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
  return useUsageIssue(option?.providerId, Boolean(option));
}
