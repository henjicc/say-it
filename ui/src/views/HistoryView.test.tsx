import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { HistoryView } from "./HistoryView";

const cmd = vi.fn();
const events = vi.hoisted(() => new Map<string, () => void>());
vi.mock("@/lib/tauri", () => ({
  CMD: {
    queryHistory: "query_history",
    confirmHistoryFinalText: "confirm_history_final_text",
    discardHistoryFinalText: "discard_history_final_text",
    retryHistoryInjection: "retry_history_injection",
    deleteHistoryEntry: "delete_history_entry",
  },
  EVT: { historyChanged: "history-changed" },
  cmd: (...args: unknown[]) => cmd(...args),
  on: vi.fn(async (event: string, handler: () => void) => {
    events.set(event, handler);
    return () => events.delete(event);
  }),
}));

describe("HistoryView", () => {
  afterEach(cleanup);
  beforeEach(() => {
    events.clear();
    cmd.mockReset();
    cmd.mockResolvedValue({ items: [{ id: "one", createdAt: 1, taskKind: "dictation", sourceText: "你好", outputText: "你好，世界", smartProcessingApplied: true, diffSegments: [], instruction: "", appName: "Notepad", processName: "notepad.exe", providerId: "fake", modelId: "fake", status: "succeeded", durationMs: 20 }], total: 1 });
  });

  it("renders observed draft differences and requires confirmation for medium confidence", async () => {
    cmd.mockResolvedValueOnce({ items: [{
      id: "observed", createdAt: 1, taskKind: "dictation", sourceText: "原文", outputText: "系统结果",
      finalText: "最终结果", finalTextConfidence: "medium", finalTextSource: "click",
      smartProcessingApplied: true,
      diffSegments: [{ kind: "delete", text: "系统" }, { kind: "insert", text: "最终" }, { kind: "equal", text: "结果" }],
      status: "succeeded", durationMs: 20,
    }], total: 1 });
    render(<HistoryView />);
    expect(await screen.findByText("最终结果")).toBeInTheDocument();
    expect(screen.getByText(/待确认/)).toBeInTheDocument();
    expect(screen.getByLabelText("最终草稿差异")).toHaveTextContent("系统最终结果");
    fireEvent.click(screen.getByRole("button", { name: "确认并学习" }));
    await waitFor(() => expect(cmd).toHaveBeenCalledWith("confirm_history_final_text", { id: "observed", finalText: "最终结果" }));
  });
  it("renders persisted history and its source application", async () => {
    render(<HistoryView />);
    expect(await screen.findByText("你好，世界")).toBeInTheDocument();
    expect(screen.getByText("Notepad")).toBeInTheDocument();
    expect(screen.getByText("识别原文")).toBeInTheDocument();
    expect(screen.getByText("你好")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "复制原文", hidden: true })).toBeInTheDocument();
    await waitFor(() => expect(cmd).toHaveBeenCalledWith("query_history", expect.anything()));
  });

  it("shows saved recognition while processing and prevents competing edits or injection", async () => {
    cmd.mockResolvedValueOnce({ items: [{ id: "pending", createdAt: 1, taskKind: "dictation", sourceText: "刚识别出的原文", outputText: "刚识别出的原文", status: "recognized" }], total: 1 });
    render(<HistoryView />);
    expect(await screen.findByText("刚识别出的原文")).toBeInTheDocument();
    expect(screen.getAllByText("原文已保存").length).toBeGreaterThan(0);
    expect(screen.getByRole("button", { name: "复制" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "修正" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "重试注入" })).toBeDisabled();
  });

  it("does not let an older raw-text query overwrite an updated result", async () => {
    let finishOld: (value: unknown) => void = () => {};
    cmd.mockImplementationOnce(() => new Promise((resolve) => { finishOld = resolve; }));
    render(<HistoryView />);
    await waitFor(() => expect(events.has("history-changed")).toBe(true));
    await act(async () => events.get("history-changed")?.());
    expect(await screen.findByText("你好，世界")).toBeInTheDocument();
    await act(async () => finishOld({ items: [], total: 0 }));
    expect(screen.getByText("你好，世界")).toBeInTheDocument();
  });
});
