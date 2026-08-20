import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { CMD, cmd } from "@/lib/tauri";
import { AssistantAnswerApp } from "./assistant";

vi.mock("@/lib/tauri", () => ({
  CMD: {
    getAssistantAnswer: "get_assistant_answer",
    closeAssistantAnswer: "close_assistant_answer",
    insertAssistantAnswer: "insert_assistant_answer",
    regenerateAssistantAnswer: "regenerate_assistant_answer",
    continueAssistantAnswer: "continue_assistant_answer",
    startAssistantFollowUpVoice: "start_assistant_follow_up_voice",
    stopAssistantFollowUpVoice: "stop_assistant_follow_up_voice",
    setAssistantAnswerPinned: "set_assistant_answer_pinned",
  },
  cmd: vi.fn(async (name: string) => name === "get_assistant_answer"
    ? { text: "## 回答正文\n\n**重点**", reasoning: "先分析问题", sourceText: "选区", canInsert: true, streaming: false, pinned: false }
    : undefined),
  on: vi.fn(async () => () => undefined),
}));

describe("AssistantAnswerApp", () => {
  beforeEach(() => vi.clearAllMocks());
  afterEach(cleanup);

  it("shows the safe answer actions and selection context", async () => {
    const { container } = render(<AssistantAnswerApp />);
    expect(container.firstElementChild).toHaveClass("assistant-answer-window");
    expect(await screen.findByText("回答正文")).toBeInTheDocument();
    expect(screen.getByText("重点")).toBeInTheDocument();
    expect(screen.getByText("思考过程")).toBeInTheDocument();
    expect(screen.getByText("选区")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /重新生成/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /插入当前位置/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "置顶窗口" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "开始语音输入" })).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "继续追问" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "发送追问" })).toBeDisabled();
  });

  it("sends a typed follow-up with Enter", async () => {
    render(<AssistantAnswerApp />);
    const input = await screen.findByRole("textbox", { name: "继续追问" });
    fireEvent.change(input, { target: { value: "再说详细一点" } });
    fireEvent.keyDown(input, { key: "Enter" });
    await waitFor(() => expect(cmd).toHaveBeenCalledWith(CMD.continueAssistantAnswer, {
      prompt: "再说详细一点",
    }));
  });

  it("uses the send button to finish voice input", async () => {
    render(<AssistantAnswerApp />);
    fireEvent.click(await screen.findByRole("button", { name: "开始语音输入" }));
    const send = await screen.findByRole("button", { name: "结束语音并发送" });
    fireEvent.click(send);
    await waitFor(() => expect(cmd).toHaveBeenCalledWith(CMD.stopAssistantFollowUpVoice));
  });
});
