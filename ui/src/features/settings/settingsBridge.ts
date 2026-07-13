import { CMD, cmd, type AppSettings, type AppSnapshot } from "@/lib/tauri";
import { hydrateDictPrefs, useDictPrefs } from "@/store/useDictPrefs";
import { hydrateSubtitlePrefs, useSubtitleStore } from "@/store/useSubtitleStore";
import { hydrateComparePrefs, useCompareStore } from "@/store/useCompareStore";
import { hydrateTheme, useThemeStore } from "@/store/useThemeStore";
import { loadModelCatalog } from "@/features/asr/modelRegistry";
import { hydrateModelOptions, DEFAULT_FILE_ASR_MODEL } from "@/features/asr/modelOptions";
import { useProviderStore } from "@/store/useProviderStore";
import { useTranscriptionStore } from "@/store/useTranscriptionStore";

function json(key: string): Record<string, unknown> | undefined {
  try { const raw = localStorage.getItem(key); return raw ? JSON.parse(raw) : undefined; } catch { return undefined; }
}

function apply(settings: AppSettings) {
  hydrateDictPrefs(settings.dictationPrefs); hydrateSubtitlePrefs(settings.subtitlePrefs);
  hydrateComparePrefs(settings.comparePrefs); hydrateTheme(settings.theme);
  // 旧 Data URL 仅作迁移兼容镜像；运行时和设置页试听均由 Rust 原生播放。
}

export async function initializeSettings(): Promise<void> {
  const catalog = await loadModelCatalog();
  hydrateModelOptions();
  useProviderStore.getState().hydrateCatalog(catalog.providers);
  if (!useTranscriptionStore.getState().params.model) {
    useTranscriptionStore.getState().setParams({ model: DEFAULT_FILE_ASR_MODEL });
  }
  await cmd(CMD.importLegacySettings, { legacy: {
    dictationPrefs: json("sayItDictPrefs") ?? useDictPrefs.getState().prefs,
    subtitlePrefs: json("sayItSubtitlePrefs") ?? useSubtitleStore.getState().prefs,
    comparePrefs: json("sayItComparePrefs") ?? useCompareStore.getState().prefs,
    theme: json("sayItAccentTheme") ?? useThemeStore.getState().theme,
    customCueStart: localStorage.getItem("dictCueStartData"), customCueEnd: localStorage.getItem("dictCueEndData"),
  }});
  apply((await cmd<AppSnapshot>(CMD.getAppSnapshot)).settings);
}
