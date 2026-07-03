import { useEffect } from "react";
import { Titlebar } from "@/components/shell/Titlebar";
import { Sidebar } from "@/components/shell/Sidebar";
import { useUiStore, type ViewKey } from "@/store/useUiStore";
import { CMD, cmd } from "@/lib/tauri";
import type { SessionStatus } from "@/store/useUiStore";
import { useTauriBridge } from "@/hooks/useTauriBridge";
import { accentContrast, accentDark, accentLight, useThemeStore } from "@/store/useThemeStore";

import { DictationView } from "@/views/DictationView";
import { RealtimeSubtitlesPanel } from "@/views/RealtimeSubtitlesPanel";
import { TranscriptionView } from "@/views/TranscriptionView";
import { SettingsView } from "@/views/SettingsView";

const VIEWS: Record<ViewKey, React.ReactNode> = {
  dictation: <DictationView />,
  subtitles: <RealtimeSubtitlesPanel />,
  transcription: <TranscriptionView />,
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
    root.style.setProperty(
      "--color-accent-contrast",
      theme.tone === "dark" ? "#FFFFFF" : accentContrast(theme.accent),
    );
    const light = theme.tone === "light";
    root.style.setProperty("--color-bg", light ? "#F4F7FB" : "#0A0E16");
    root.style.setProperty("--color-bg-sidebar", light ? "#EAF0F8" : "#080B12");
    root.style.setProperty("--color-bg-titlebar", light ? "#EAF0F8" : "#080B12");
    root.style.setProperty("--color-overlay", light ? "#FFFFFF" : "#12161F");
    root.style.setProperty("--color-fg", light ? "#111827" : "#FFFFFF");
    root.style.setProperty("--color-fg-muted", light ? "rgba(17, 24, 39, 0.68)" : "rgba(255, 255, 255, 0.64)");
    root.style.setProperty("--color-fg-subtle", light ? "rgba(17, 24, 39, 0.42)" : "rgba(255, 255, 255, 0.42)");
    root.style.setProperty("--color-fg-faint", light ? "rgba(17, 24, 39, 0.32)" : "rgba(255, 255, 255, 0.30)");
    root.style.setProperty("--color-surface", light ? "rgba(255, 255, 255, 0.76)" : "rgba(255, 255, 255, 0.035)");
    root.style.setProperty("--color-surface-hover", light ? "rgba(255, 255, 255, 0.92)" : "rgba(255, 255, 255, 0.06)");
    root.style.setProperty("--color-surface-strong", light ? "rgba(255, 255, 255, 0.92)" : "rgba(255, 255, 255, 0.08)");
    root.style.setProperty("--color-line", light ? "rgba(17, 24, 39, 0.1)" : "rgba(255, 255, 255, 0.08)");
    root.style.setProperty("--color-line-strong", light ? "rgba(17, 24, 39, 0.18)" : "rgba(255, 255, 255, 0.16)");
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
        <main className="min-h-0 flex-1 overflow-y-auto px-9 py-8">
          <div className="mx-auto w-full max-w-[1180px]">{VIEWS[view]}</div>
        </main>
      </div>
    </div>
  );
}
