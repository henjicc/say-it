import type { ProviderProfile } from "@/store/useProviderStore";

/** 需要填写密钥或设置的供应商才在模型页单独列出；内置 OCR、本机识别等无需配置的不列。 */
export function isManagedProvider(profile: ProviderProfile): boolean {
  return profile.enabled && (profile.authKind !== "none" || Boolean(profile.configFields?.length));
}

export function providerReady(profile: ProviderProfile): boolean {
  return profile.authKind === "none" || Boolean(profile.status?.configured || profile.status?.hasApiKey);
}

function visibilityOf(profile: ProviderProfile): Record<string, boolean> {
  const value = profile.config?.modelVisibility;
  return value && typeof value === "object" ? value as Record<string, boolean> : {};
}

/** 模型是否出现在用途下拉框里；用户没有设过时按 shownByDefault。 */
export function isModelVisible(profile: ProviderProfile, modelId: string, shownByDefault: boolean): boolean {
  const stored = visibilityOf(profile)[modelId];
  return typeof stored === "boolean" ? stored : shownByDefault;
}

export function modelVisibilityPatch(profile: ProviderProfile, modelId: string, visible: boolean) {
  return { modelVisibility: { ...visibilityOf(profile), [modelId]: visible } };
}

/** 用途下拉框能否列出该模型：供应商无需配置（本机、插件模型包）时总能列出。 */
export function isModelOffered(profile: ProviderProfile | undefined, modelId: string, shownByDefault: boolean): boolean {
  if (!profile || !isManagedProvider(profile)) return true;
  return providerReady(profile) && isModelVisible(profile, modelId, shownByDefault);
}
