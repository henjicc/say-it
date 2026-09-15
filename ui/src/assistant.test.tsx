import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { CMD, cmd } from "@/lib/tauri";
import { AssistantAnswerApp, buildVoiceWaveTargets, VOICE_WAVE_BAR_COUNT } from "./assistant";

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

  /// 生成中断时后端会把已经生成出来的正文连同错误一起下发（`processed + error`
  /// 这条路径本来就存在）。原来的 `error ? 错误 : text` 二选一渲染会把用户已经
  /// 读到一半的回答整段抹掉，而且没有任何恢复入口。
  it("keeps the partial answer visible alongside the error", async () => {
    vi.mocked(cmd).mockImplementation(async (name: string) =>
      name === "get_assistant_answer"
        ? {
            text: "## 已生成的前半段",
            reasoning: "",
            sourceText: "",
            canInsert: false,
            streaming: false,
            pinned: false,
            error: "网络连接中断",
          }
        : undefined);

    render(<AssistantAnswerApp />);

    expect(await screen.findByText("已生成的前半段")).toBeInTheDocument();
    expect(screen.getByText("网络连接中断")).toBeInTheDocument();
    // 正文还在，复制按钮就该可用。
    expect(screen.getByRole("button", { name: /复制/ })).toBeEnabled();
  });

  it("falls back to the error alone when nothing was generated", async () => {
    vi.mocked(cmd).mockImplementation(async (name: string) =>
      name === "get_assistant_answer"
        ? {
            text: "",
            reasoning: "",
            sourceText: "",
            canInsert: false,
            streaming: false,
            pinned: false,
            error: "未读取到选区",
          }
        : undefined);

    render(<AssistantAnswerApp />);

    expect(await screen.findByText("未读取到选区")).toBeInTheDocument();
    expect(screen.queryByText("正在等待回答…")).not.toBeInTheDocument();
  });

  it("builds a fixed symmetric waveform without saturating every bar", () => {
    const targets = buildVoiceWaveTargets({ level: 1, peaks: [1, 1, 1, 1, 1, 1] });
    expect(targets).toHaveLength(VOICE_WAVE_BAR_COUNT);
    expect(Math.max(...targets)).toBeLessThan(0.85);
    for (let index = 0; index < targets.length; index += 1) {
      expect(targets[index]).toBeCloseTo(targets[targets.length - 1 - index], 6);
    }
  });
});
