import { useEffect, useState } from "react";
import { Titlebar } from "@/components/shell/Titlebar";
import { Sidebar } from "@/components/shell/Sidebar";
import { useUiStore, type ViewKey } from "@/store/useUiStore";
import { CMD, EVT, cmd, on } from "@/lib/tauri";
import type { SessionStatus } from "@/store/useUiStore";
import { useTauriBridge } from "@/hooks/useTauriBridge";
import { applySystemGlassToDocument, applyThemeToDocument, useThemeStore } from "@/store/useThemeStore";
import { useFloatingOrbStore } from "@/store/useFloatingOrbStore";
import { initializeSettings } from "@/features/settings/settingsBridge";

import { DictationView } from "@/views/DictationView";
import { HomeView } from "@/views/HomeView";
import { VoiceAssistantView } from "@/views/VoiceAssistantPanel";
import { RealtimeSubtitlesPanel } from "@/views/RealtimeSubtitlesPanel";
import { TranscriptionView } from "@/views/TranscriptionView";
import { CustomizationView } from "@/views/CustomizationView";
import { SettingsView } from "@/views/SettingsView";
import { HistoryView } from "@/views/HistoryView";
import { AboutDialog } from "@/views/AboutView";
import { PluginDropInstaller } from "@/components/PluginDropInstaller";
import { ShortcutConflictDialog } from "@/features/hotkeys/ShortcutConflictDialog";
import { OnboardingWizard } from "@/components/OnboardingWizard";
import type { SetupStatus } from "@/lib/tauri";

const VIEWS: Record<ViewKey, React.ReactNode> = {
  home: <HomeView />,
  dictation: <DictationView />,
  assistant: <VoiceAssistantView />,
  subtitles: <RealtimeSubtitlesPanel />,
  transcription: <TranscriptionView />,
  customization: <CustomizationView />,
  history: <HistoryView />,
  settings: <SettingsView />,
};

export default function App() {
  const view = useUiStore((s) => s.view);
  const aboutOpen = useUiStore((s) => s.aboutOpen);
  const closeAbout = useUiStore((s) => s.closeAbout);
  const setSession = useUiStore((s) => s.setSession);
  const setView = useUiStore((s) => s.setView);
  const theme = useThemeStore((s) => s.theme);
  const systemGlass = useFloatingOrbStore((s) => s.settings);
  const [settingsReady, setSettingsReady] = useState(false);
  const [setupOpen, setSetupOpen] = useState(false);

  const bridgeReady = useTauriBridge();

  useEffect(() => {
    void initializeSettings()
      .catch((error) => console.error("应用目录与设置初始化失败", error))
      .finally(() => setSettingsReady(true));
  }, []);

  useEffect(() => {
    if (!settingsReady || !bridgeReady) return;
    void cmd(CMD.mainWindowReady).catch((error) => {
      console.error("主窗口 ready 握手失败", error);
    });
  }, [bridgeReady, settingsReady]);

  useEffect(() => {
    applyThemeToDocument(theme);
  }, [theme]);

  useEffect(() => {
    applySystemGlassToDocument(systemGlass);
  }, [systemGlass.glassEnabled, systemGlass.glassTint]);

  useEffect(() => {
    cmd<SessionStatus>(CMD.getSessionStatus)
      .then((status) => setSession(status))
      .catch(() => {});
  }, [setSession]);

  useEffect(() => {
    if (!settingsReady) return;
    void cmd<SetupStatus>(CMD.getSetupStatus).then((status) => setSetupOpen(!status.complete)).catch(() => {});
    const open = () => setSetupOpen(true);
    window.addEventListener("sayit-open-setup", open);
    return () => window.removeEventListener("sayit-open-setup", open);
  }, [settingsReady]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    void on(EVT.openHistory, () => setView("history")).then((value) => { unlisten = value; });
    return () => unlisten?.();
  }, [setView]);

  return (
    <div className="flex h-full w-full flex-col overflow-hidden bg-[var(--color-bg)] text-[var(--color-fg)]">
      {!settingsReady ? null : <>
      <Titlebar />
      <div className="relative flex min-h-0 flex-1">
        <Sidebar />
        <main className="min-h-0 flex-1 overflow-y-auto px-9 py-8">
          <div className="mx-auto w-full max-w-[1180px]">{VIEWS[view]}</div>
        </main>
        <AboutDialog open={aboutOpen} onClose={closeAbout} />
        <PluginDropInstaller />
        <ShortcutConflictDialog />
        <OnboardingWizard open={setupOpen} onClose={() => setSetupOpen(false)} />
      </div>
      </>}
    </div>
  );
}
