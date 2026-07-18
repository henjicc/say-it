import { loadModelCatalog } from "@/features/asr/modelRegistry";
import { hydrateModelOptions } from "@/features/asr/modelOptions";
import { CMD, cmd } from "@/lib/tauri";
import { useProviderStore } from "@/store/useProviderStore";

export interface PluginSummary {
  id: string;
  name: string;
  version: string;
  providerId: string;
  permissions: string[];
  models: string[];
  trust: "trusted" | "signed-untrusted" | "integrity-only" | "unsigned";
  actions: string[];
  hasBrowserSession: boolean;
  enabled: boolean;
  runtimeKind: "javascript" | "model-pack";
  modelPack?: {
    engine: string;
    state: "pending" | "partial" | "corrupt" | "downloading" | "ready";
    totalBytes: number;
    readyBytes: number;
    downloadable: boolean;
  };
}

export interface PluginSnapshot {
  apiVersion: number;
  plugins: PluginSummary[];
  errors: { path: string; message: string }[];
}

export function requiresExplicitTrust(error: unknown) {
  const reason = String(error);
  return reason.includes("未签名") || reason.includes("尚未受信任");
}

export async function refreshPluginConsumers() {
  await loadModelCatalog();
  hydrateModelOptions();
  await useProviderStore.getState().load();
}

export async function installPluginPackage(
  sourcePath: string,
  options: { allowUnsigned: boolean; trustSigningKey: boolean },
) {
  const snapshot = await cmd<PluginSnapshot>(CMD.installProviderPlugin, {
    sourcePath,
    ...options,
  });
  await refreshPluginConsumers();
  return snapshot;
}
