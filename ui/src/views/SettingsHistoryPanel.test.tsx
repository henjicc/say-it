import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { SettingsHistoryPanel } from "./SettingsHistoryPanel";

const cmd = vi.fn();
vi.mock("@/lib/tauri", () => ({
  CMD: {
    getAppSnapshot: "get_app_snapshot",
    updateAppSettings: "update_app_settings",
    clearHistory: "clear_history",
    clearUsageSummary: "clear_usage_summary",
    clearLearningMemory: "clear_learning_memory",
  },
  cmd: (...args: unknown[]) => cmd(...args),
}));

describe("SettingsHistoryPanel", () => {
  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });
  beforeEach(() => {
    cmd.mockReset();
    cmd.mockResolvedValue({
      settings: {
        historyPrefs: {
          enabled: true,
          retentionDays: 30,
          excludedApps: [],
          finalDraftObservationEnabled: false,
          correctionLearningEnabled: false,
          cloudLearningContextEnabled: false,
          learningMemoryRetentionDays: 180,
        },
      },
    });
  });

  it("keeps observation and learning off until the user enables both", async () => {
    render(<SettingsHistoryPanel />);
    const observation = await screen.findByRole("switch", { name: "记录发送前修改" });
    const learning = screen.getByRole("switch", { name: "个性化纠错" });
    expect(observation).not.toBeChecked();
    expect(learning).toBeDisabled();
    fireEvent.click(observation);
    await waitFor(() => expect(cmd).toHaveBeenCalledWith("update_app_settings", {
      domain: "history",
      value: expect.objectContaining({ finalDraftObservationEnabled: true, correctionLearningEnabled: false }),
    }));
    await waitFor(() => expect(learning).toBeEnabled());
    fireEvent.click(learning);
    await waitFor(() => expect(cmd).toHaveBeenCalledWith("update_app_settings", {
      domain: "history",
      value: expect.objectContaining({ finalDraftObservationEnabled: true, correctionLearningEnabled: true }),
    }));
  });

  it("requires explicit confirmation before sending learning context to cloud models", async () => {
    cmd.mockResolvedValueOnce({
      settings: { historyPrefs: {
        enabled: true,
        retentionDays: 30,
        excludedApps: [],
        finalDraftObservationEnabled: true,
        correctionLearningEnabled: true,
        cloudLearningContextEnabled: false,
        learningMemoryRetentionDays: 180,
      } },
    });
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(false);
    render(<SettingsHistoryPanel />);
    const cloud = await screen.findByRole("switch", { name: "云端参考学习记录" });
    fireEvent.click(cloud);
    expect(confirm).toHaveBeenCalledTimes(1);
    expect(cmd).not.toHaveBeenCalledWith("update_app_settings", expect.objectContaining({
      value: expect.objectContaining({ cloudLearningContextEnabled: true }),
    }));
    confirm.mockReturnValue(true);
    fireEvent.click(cloud);
    await waitFor(() => expect(cmd).toHaveBeenCalledWith("update_app_settings", {
      domain: "history",
      value: expect.objectContaining({ cloudLearningContextEnabled: true }),
    }));
  });
});
