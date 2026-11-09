import { create } from "zustand";
import { CMD, cmd } from "@/lib/tauri";
import { useUiStore } from "@/store/useUiStore";

/** 开发构建始终开启；发布构建按 `diagnosticsPrefs.developerMode`，在「关于」里连点图标打开。 */
interface DeveloperState {
  enabled: boolean;
  setEnabled: (enabled: boolean) => Promise<void>;
}

/** 后端按字段合并诊断设置，只传要改的开关。 */
export async function updateDiagnosticsPrefs(patch: Record<string, unknown>): Promise<void> {
  await cmd(CMD.updateAppSettings, { domain: "diagnostics", value: patch });
}

function applyEnabled(enabled: boolean) {
  useDeveloperStore.setState({ enabled });
  // 关掉时正停在开发者页，回到听写。
  if (!enabled && useUiStore.getState().view === "developer") useUiStore.getState().setView("dictation");
}

export const useDeveloperStore = create<DeveloperState>(() => ({
  enabled: import.meta.env.DEV,
  setEnabled: async (enabled) => {
    await updateDiagnosticsPrefs({ developerMode: enabled });
    applyEnabled(import.meta.env.DEV || enabled);
  },
}));

export function hydrateDeveloperMode(prefs: Record<string, unknown>) {
  applyEnabled(import.meta.env.DEV || prefs.developerMode === true);
}
