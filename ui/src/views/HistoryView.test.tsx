import { render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { HistoryView } from "./HistoryView";

const cmd = vi.fn();
vi.mock("@/lib/tauri", () => ({
  CMD: { queryHistory: "query_history", updateHistoryText: "update_history_text", deleteHistoryEntry: "delete_history_entry" },
  EVT: { historyChanged: "history-changed" },
  cmd: (...args: unknown[]) => cmd(...args),
  on: vi.fn(async () => () => undefined),
}));

describe("HistoryView", () => {
  beforeEach(() => cmd.mockResolvedValue({ items: [{ id: "one", createdAt: 1, taskKind: "dictation", sourceText: "你好", outputText: "你好，世界", instruction: "", appName: "Notepad", processName: "notepad.exe", providerId: "fake", modelId: "fake", status: "succeeded", durationMs: 20 }], total: 1 }));
  it("renders persisted history and its source application", async () => {
    render(<HistoryView />);
    expect(await screen.findByText("你好，世界")).toBeInTheDocument();
    expect(screen.getByText("Notepad")).toBeInTheDocument();
    await waitFor(() => expect(cmd).toHaveBeenCalledWith("query_history", expect.anything()));
  });
});
