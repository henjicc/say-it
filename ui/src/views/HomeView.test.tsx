import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { HomeView } from "./HomeView";

vi.mock("@/lib/tauri", () => ({
  CMD: { getSetupStatus: "get_setup_status", getUsageSummary: "get_usage_summary" },
  EVT: { historyChanged: "history_changed" },
  cmd: vi.fn(async (name: string) => {
    if (name === "get_setup_status") return { checks: [] };
    if (name === "get_usage_summary") return { successfulActions: 0, outputChars: 0, spokenDurationMs: 0, estimatedTimeSavedMs: 0 };
    return undefined;
  }),
  cmdSilent: vi.fn(),
  on: vi.fn(async () => () => {}),
}));

vi.mock("@/features/asr/modelRegistry", () => ({
  useModelCatalogRevision: () => 1,
  ocrOptionsForScene: () => [],
}));
vi.mock("@/features/asr/modelOptions", () => ({
  DEFAULT_REALTIME_ASR_MODEL: "apple-speech",
  DICTATION_ASR_MODEL_OPTIONS: [{ value: "apple-speech", label: "Apple 系统语音识别" }],
  isSupportedDictationModel: () => true,
}));
vi.mock("@/features/dictation/ShortcutRecorder", () => ({ ShortcutRecorder: () => <div>快捷键录入</div> }));
vi.mock("@/features/hotkeys/catalog", () => ({
  loadShortcutBindings: vi.fn(async () => []),
  shortcutTargetKey: () => "dictation-main",
  updateShortcutBinding: vi.fn(),
}));

describe("HomeView", () => {
  afterEach(cleanup);

  it("uses the compact aligned quick-settings layout", async () => {
    render(<HomeView />);

    expect(screen.queryByText("开口输入，也能编辑、翻译和问答")).not.toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "快速设置" })).toBeInTheDocument();
    expect(screen.getByText("主语音识别模型")).toBeInTheDocument();
    expect(screen.getByText("全局默认智能模型")).toBeInTheDocument();
    expect(screen.getByText("环境状态")).toBeInTheDocument();
    expect(screen.queryByText("智能优化和智能助手默认跟随此设置。")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "环境状态" })).toBeInTheDocument();
    expect(await screen.findByRole("combobox", { name: "语音输入触发方式" })).toHaveTextContent("单击切换");
    expect(screen.getByText(/正在检查环境…|所有关键能力均可用/)).toBeInTheDocument();
  });
});
