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
  },
  cmd: (...args: unknown[]) => cmd(...args),
}));

describe("SettingsHistoryPanel", () => {
  afterEach(cleanup);
  beforeEach(() => {
    cmd.mockReset();
    cmd.mockResolvedValue({
      settings: {
        historyPrefs: {
          enabled: true,
          retentionDays: 30,
          excludedApps: [],
          finalDraftLearningEnabled: false,
        },
      },
    });
  });

  it("keeps final draft learning off until the user enables it", async () => {
    render(<SettingsHistoryPanel />);
    const toggle = await screen.findByRole("switch", { name: "学习最终草稿" });
    expect(toggle).not.toBeChecked();
    fireEvent.click(toggle);
    await waitFor(() => expect(cmd).toHaveBeenCalledWith("update_app_settings", {
      domain: "history",
      value: expect.objectContaining({ finalDraftLearningEnabled: true }),
    }));
  });
});
