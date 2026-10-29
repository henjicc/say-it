import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";

const { invoke, initializeSettings, state } = vi.hoisted(() => ({
  invoke: vi.fn(),
  initializeSettings: vi.fn(),
  state: {
    view: "home", aboutOpen: false, closeAbout: vi.fn(), setSession: vi.fn(), setView: vi.fn(),
    theme: {}, settings: { glassEnabled: false, glassTint: "" },
  },
}));

vi.mock("@/lib/tauri", () => ({
  CMD: { getSetupStatus: "get_setup_status", getSessionStatus: "get_session_status", mainWindowReady: "main_window_ready" },
  EVT: { openHistory: "open_history" },
  cmd: invoke,
  on: vi.fn().mockResolvedValue(() => {}),
}));
vi.mock("@/hooks/useTauriBridge", () => ({ useTauriBridge: () => true }));
vi.mock("@/features/settings/settingsBridge", () => ({ initializeSettings }));
vi.mock("@/store/useUiStore", () => ({ useUiStore: (select: (value: typeof state) => unknown) => select(state) }));
vi.mock("@/store/useThemeStore", () => ({
  useThemeStore: (select: (value: typeof state) => unknown) => select(state),
  applyThemeToDocument: vi.fn(), applySystemGlassToDocument: vi.fn(),
}));
vi.mock("@/store/useFloatingOrbStore", () => ({ useFloatingOrbStore: (select: (value: typeof state) => unknown) => select(state) }));
vi.mock("@/components/shell/Titlebar", () => ({ Titlebar: () => null }));
vi.mock("@/components/shell/Sidebar", () => ({ Sidebar: () => null }));
vi.mock("@/views/DictationView", () => ({ DictationView: () => null }));
vi.mock("@/views/HomeView", () => ({ HomeView: () => <div>应用主页</div> }));
vi.mock("@/views/VoiceAssistantPanel", () => ({ VoiceAssistantView: () => null }));
vi.mock("@/views/RealtimeSubtitlesPanel", () => ({ RealtimeSubtitlesPanel: () => null }));
vi.mock("@/views/TranscriptionView", () => ({ TranscriptionView: () => null }));
vi.mock("@/views/CustomizationView", () => ({ CustomizationView: () => null }));
vi.mock("@/views/SettingsView", () => ({ SettingsView: () => null }));
vi.mock("@/views/HistoryView", () => ({ HistoryView: () => null }));
vi.mock("@/views/AboutView", () => ({ AboutDialog: () => null }));
vi.mock("@/components/PluginDropInstaller", () => ({ PluginDropInstaller: () => null }));
vi.mock("@/features/hotkeys/ShortcutConflictDialog", () => ({ ShortcutConflictDialog: () => null }));
vi.mock("@/components/OnboardingWizard", () => ({
  OnboardingWizard: ({ open, onClose }: { open: boolean; onClose: () => void }) => open
    ? <div role="dialog"><button onClick={onClose}>关闭引导</button></div> : null,
}));

describe("startup onboarding", () => {
  afterEach(cleanup);
  beforeEach(() => {
    invoke.mockReset();
    initializeSettings.mockReset().mockResolvedValue(undefined);
  });

  it.each([
    { onboardingVersion: 0, requiredVersion: 1, complete: false, opens: true },
    { onboardingVersion: 1, requiredVersion: 1, complete: true, opens: false },
    { onboardingVersion: 1, requiredVersion: 2, complete: false, opens: false },
  ])("opens only for a new user: $onboardingVersion / $requiredVersion", async ({ opens, ...status }) => {
    invoke.mockImplementation(async (name: string) => name === "get_setup_status" ? { ...status, checks: [] } : {});
    const first = render(<App />);
    await screen.findByText("应用主页");
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("get_setup_status"));
    expect(Boolean(screen.queryByRole("dialog"))).toBe(opens);

    if (!opens) {
      // 模拟完整重挂载，确认判断来自后端快照，而不是当前页面内存。
      first.unmount();
      render(<App />);
      await screen.findByText("应用主页");
      await waitFor(() => expect(invoke.mock.calls.filter(([name]) => name === "get_setup_status")).toHaveLength(2));
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    }
    fireEvent(window, new Event("sayit-open-setup"));
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "关闭引导" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });
});
