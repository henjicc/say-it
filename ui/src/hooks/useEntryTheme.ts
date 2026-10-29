import { useEffect } from "react";
import { useTauriEvent } from "@/hooks/useTauriEvent";
import { CMD, EVT, cmd, type AppSnapshot } from "@/lib/tauri";
import { applyThemeToDocument, type AccentTheme } from "@/store/useThemeStore";

/**
 * 给主窗口以外的独立入口初始化主题，并跟随后续的主题变更。
 *
 * 主题是运行时切换的：亮色令牌靠 `<html data-ui-tone="light">` 生效，强调色靠覆写
 * `--color-*` 生效，而这两件事只有 `App.tsx` 在做。任何不初始化主题的入口都会一直按
 * 暗色默认值渲染——亮色主题下整窗错色，强调色也不跟随。
 *
 * 每个独立入口都是自己的 document，所以初始化必须逐窗做一次，不能靠主窗口代劳。
 */
export function useEntryTheme() {
  useEffect(() => {
    void cmd<AppSnapshot>(CMD.getAppSnapshot)
      .then((snapshot) => applyThemeToDocument(snapshot.settings.theme as Partial<AccentTheme>))
      .catch(() => undefined);
  }, []);
  useTauriEvent<Partial<AccentTheme>>(EVT.themeChanged, applyThemeToDocument);
}
