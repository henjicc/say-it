import { useEffect, useState } from "react";
import { Titlebar } from "@/components/shell/Titlebar";
import { Sidebar } from "@/components/shell/Sidebar";
import { useUiStore, type ViewKey } from "@/store/useUiStore";
import { Button } from "@/components/ui/Button";
import { cn } from "@/lib/cn";
import { CMD, EVT, cmd, on } from "@/lib/tauri";
import type { SessionStatus } from "@/store/useUiStore";
import { useTauriBridge } from "@/hooks/useTauriBridge";
import { applySystemGlassToDocument, applyThemeToDocument, useThemeStore } from "@/store/useThemeStore";
import { useFloatingOrbStore } from "@/store/useFloatingOrbStore";
import { initializeSettings } from "@/features/settings/settingsBridge";

import { DictationPage } from "@/views/dictation/DictationPage";
import { AssistantPage } from "@/views/assistant/AssistantPage";
import { SubtitlesPage } from "@/views/subtitles/SubtitlesPage";
import { TranscriptionPage } from "@/views/transcription/TranscriptionPage";
import { ModelsPage } from "@/views/models/ModelsPage";
import { LexiconPage } from "@/views/lexicon/LexiconPage";
import { HistoryPage } from "@/views/history/HistoryPage";
import { SettingsPage } from "@/views/settings/SettingsPage";
import { SimpleDictation, SimpleHistory, SimpleSettings, SimpleSubtitles, SimpleTranscription } from "@/views/simple/SimplePages";
import { useLayoutMode } from "@/hooks/useLayoutMode";
import { AboutDialog } from "@/views/AboutView";
import { PluginDropInstaller } from "@/components/PluginDropInstaller";
import { ShortcutConflictDialog } from "@/features/hotkeys/ShortcutConflictDialog";
import { OnboardingWizard } from "@/components/OnboardingWizard";
import type { SetupStatus } from "@/lib/tauri";

const PRO_VIEWS: Record<ViewKey, React.ReactNode> = {
  dictation: <DictationPage />,
  assistant: <AssistantPage />,
  subtitles: <SubtitlesPage />,
  transcription: <TranscriptionPage />,
  models: <ModelsPage />,
  lexicon: <LexiconPage />,
  history: <HistoryPage />,
  settings: <SettingsPage />,
};

/** 简单模式只有这几页；停在专业模式独有的页面时，窄窗口先显示听写。 */
const SIMPLE_VIEWS: Partial<Record<ViewKey, React.ReactNode>> = {
  dictation: <SimpleDictation />,
  subtitles: <SimpleSubtitles />,
  transcription: <SimpleTranscription />,
  history: <SimpleHistory />,
  settings: <SimpleSettings />,
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
  const [initError, setInitError] = useState("");

  const bridgeReady = useTauriBridge();
  const layoutMode = useUiStore((s) => s.layoutMode);
  useLayoutMode();

  useEffect(() => {
    // 初始化失败不能当作正常启动。这里是前端唯一一次从 Rust 拉取权威配置的地方，
    // 失败意味着各个 store 停在空默认值上；照常渲染的话，用户一次正常编辑就会把
    // 默认值整份写回后端，覆盖掉磁盘上的真实配置（热词与上下文没有 localStorage
    // 镜像，覆盖后不可恢复）。所以要把错误显式呈现出来，而不是只写 console。
    void initializeSettings()
      .catch((error: unknown) => setInitError(String(error)))
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
    // 引导版本升级不能撤销用户已关闭的选择；后续只通过设置页手动打开。
    void cmd<SetupStatus>(CMD.getSetupStatus).then((status) => setSetupOpen(status.onboardingVersion === 0)).catch(() => {});
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
    <div className="flex h-full w-full flex-col overflow-hidden bg-[var(--color-bg-sidebar)] text-[var(--color-fg)]">
      {!settingsReady ? null : <>
      <Titlebar />
      {initError && (
        <div
          role="alert"
          className="flex flex-none items-start gap-3 border-b border-[var(--color-line)] bg-[color-mix(in_srgb,var(--color-err)_10%,transparent)] px-9 py-3"
        >
          <div className="min-w-0 flex-1 text-xs leading-relaxed text-[var(--color-err)]">
            应用设置加载失败，当前显示的可能不是你的真实配置。
            <span className="opacity-80">请先重启应用；在此之前请勿修改设置，以免覆盖磁盘上的配置。</span>
            <span className="mt-1 block break-all font-mono opacity-70">{initError}</span>
          </div>
          <Button size="sm" variant="primary" onClick={() => void cmd(CMD.restartApp)}>
            立即重启
          </Button>
        </div>
      )}
      <div className="relative flex min-h-0 flex-1">
        {layoutMode === "pro" && <Sidebar />}
        <div className={cn(
          "flex min-h-0 min-w-0 flex-1 overflow-hidden border-t border-[var(--color-line)] bg-[var(--color-bg)]",
          layoutMode === "pro" ? "rounded-tl-[10px] border-l" : "flex-col rounded-t-[10px]",
        )}>
          {layoutMode === "pro" ? PRO_VIEWS[view] : SIMPLE_VIEWS[view] ?? SIMPLE_VIEWS.dictation}
        </div>
        <AboutDialog open={aboutOpen} onClose={closeAbout} />
        <PluginDropInstaller />
        <ShortcutConflictDialog />
        <OnboardingWizard open={setupOpen} onClose={() => setSetupOpen(false)} />
      </div>
      </>}
    </div>
  );
}
