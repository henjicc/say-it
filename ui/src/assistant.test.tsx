import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { AssistantAnswerApp } from "./assistant";

vi.mock("@/lib/tauri", () => ({
  CMD: {
    getAssistantAnswer: "get_assistant_answer",
    closeAssistantAnswer: "close_assistant_answer",
    insertAssistantAnswer: "insert_assistant_answer",
    regenerateAssistantAnswer: "regenerate_assistant_answer",
  },
  cmd: vi.fn(async (name: string) => name === "get_assistant_answer"
    ? { text: "## 回答正文\n\n**重点**", reasoning: "先分析问题", sourceText: "选区", canInsert: true, streaming: false }
    : undefined),
  on: vi.fn(async () => () => undefined),
}));

describe("AssistantAnswerApp", () => {
  it("shows the safe answer actions and selection context", async () => {
    const { container } = render(<AssistantAnswerApp />);
    expect(container.firstElementChild).toHaveClass("assistant-answer-window");
    expect(await screen.findByText("回答正文")).toBeInTheDocument();
    expect(screen.getByText("重点")).toBeInTheDocument();
    expect(screen.getByText("思考过程")).toBeInTheDocument();
    expect(screen.getByText("选区")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /重新生成/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /插入当前位置/ })).toBeInTheDocument();
  });
});
