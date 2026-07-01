import { useEffect } from "react";
import { Titlebar } from "@/components/shell/Titlebar";
import { Sidebar } from "@/components/shell/Sidebar";
import { useUiStore, type ViewKey } from "@/store/useUiStore";
import { CMD, cmd } from "@/lib/tauri";
import type { SessionStatus } from "@/store/useUiStore";
import { useTauriBridge } from "@/hooks/useTauriBridge";

import { DictationView } from "@/views/DictationView";
import { SettingsView } from "@/views/SettingsView";

const VIEWS: Record<ViewKey, React.ReactNode> = {
  dictation: <DictationView />,
  settings: <SettingsView />,
};

export default function App() {
  const view = useUiStore((s) => s.view);
  const setSession = useUiStore((s) => s.setSession);

  useTauriBridge();

  useEffect(() => {
    cmd<SessionStatus>(CMD.getSessionStatus)
      .then((status) => setSession(status))
      .catch(() => {});
  }, [setSession]);

  return (
    <div className="flex h-full w-full flex-col overflow-hidden bg-black text-white">
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
