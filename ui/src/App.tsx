import { useEffect } from "react";
import { Titlebar } from "@/components/shell/Titlebar";
import { Sidebar } from "@/components/shell/Sidebar";
import { useUiStore, type ViewKey } from "@/store/useUiStore";
import { CMD, cmd } from "@/lib/tauri";
import type { SessionStatus } from "@/store/useUiStore";
import { useTauriBridge } from "@/hooks/useTauriBridge";
import { accentContrast, accentDark, accentLight, useThemeStore } from "@/store/useThemeStore";

import { DictationView } from "@/views/DictationView";
import { SettingsView } from "@/views/SettingsView";

const VIEWS: Record<ViewKey, React.ReactNode> = {
  dictation: <DictationView />,
  settings: <SettingsView />,
};

export default function App() {
  const view = useUiStore((s) => s.view);
  const setSession = useUiStore((s) => s.setSession);
  const theme = useThemeStore((s) => s.theme);

  useTauriBridge();

  useEffect(() => {
    const root = document.documentElement;
    root.dataset.uiTone = theme.tone;
    root.style.setProperty("--color-accent", theme.accent);
    root.style.setProperty("--color-accent-light", accentLight(theme.accent));
    root.style.setProperty("--color-accent-dark", accentDark(theme.accent));
    root.style.setProperty("--color-accent-contrast", accentContrast(theme.accent));
    root.style.setProperty("--color-bg", theme.tone === "light" ? "#F4F7FB" : "#000000");
    root.style.setProperty("--color-fg", theme.tone === "light" ? "#111827" : "#FFFFFF");
    root.style.setProperty("--color-fg-muted", theme.tone === "light" ? "rgba(17, 24, 39, 0.68)" : "rgba(255, 255, 255, 0.7)");
    root.style.setProperty("--color-fg-subtle", theme.tone === "light" ? "rgba(17, 24, 39, 0.42)" : "rgba(255, 255, 255, 0.4)");
    root.style.setProperty("--color-surface", theme.tone === "light" ? "rgba(255, 255, 255, 0.76)" : "rgba(255, 255, 255, 0.05)");
    root.style.setProperty("--color-surface-strong", theme.tone === "light" ? "rgba(255, 255, 255, 0.92)" : "rgba(255, 255, 255, 0.08)");
    root.style.setProperty("--color-line", theme.tone === "light" ? "rgba(17, 24, 39, 0.1)" : "rgba(255, 255, 255, 0.1)");
  }, [theme]);

  useEffect(() => {
    cmd<SessionStatus>(CMD.getSessionStatus)
      .then((status) => setSession(status))
      .catch(() => {});
  }, [setSession]);

  return (
    <div className="flex h-full w-full flex-col overflow-hidden bg-[var(--color-bg)] text-[var(--color-fg)]">
      <Titlebar />
      <div className="flex min-h-0 flex-1">
        <Sidebar />
        <main className="min-h-0 flex-1 overflow-y-auto px-8 py-6">
          <div className="mx-auto w-full max-w-5xl">{VIEWS[view]}</div>
        </main>
      </div>
    </div>
  );
}
