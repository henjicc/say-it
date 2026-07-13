import { CMD, cmd, type AppSettings, type AppSnapshot } from "@/lib/tauri";
import { hydrateDictPrefs, useDictPrefs } from "@/store/useDictPrefs";
import { hydrateSubtitlePrefs, useSubtitleStore } from "@/store/useSubtitleStore";
import { hydrateComparePrefs, useCompareStore } from "@/store/useCompareStore";
import { hydrateTheme, useThemeStore } from "@/store/useThemeStore";

function json(key: string): Record<string, unknown> | undefined {
  try { const raw = localStorage.getItem(key); return raw ? JSON.parse(raw) : undefined; } catch { return undefined; }
}

function apply(settings: AppSettings) {
  hydrateDictPrefs(settings.dictationPrefs); hydrateSubtitlePrefs(settings.subtitlePrefs);
  hydrateComparePrefs(settings.comparePrefs); hydrateTheme(settings.theme);
  // 播放仍由 Web Audio 兼容路径完成；旧 Data URL 保留到 3.1 接入原生文件播放。
}

export async function initializeSettings(): Promise<void> {
  await cmd(CMD.importLegacySettings, { legacy: {
    dictationPrefs: json("sayItDictPrefs") ?? useDictPrefs.getState().prefs,
    subtitlePrefs: json("sayItSubtitlePrefs") ?? useSubtitleStore.getState().prefs,
    comparePrefs: json("sayItComparePrefs") ?? useCompareStore.getState().prefs,
    theme: json("sayItAccentTheme") ?? useThemeStore.getState().theme,
    customCueStart: localStorage.getItem("dictCueStartData"), customCueEnd: localStorage.getItem("dictCueEndData"),
  }});
  apply((await cmd<AppSnapshot>(CMD.getAppSnapshot)).settings);
}
