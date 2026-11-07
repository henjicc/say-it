import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const cmd = vi.fn();
vi.mock("@/lib/tauri", () => ({
  CMD: new Proxy({}, { get: (_target, key) => String(key) }),
  cmd: (...args: unknown[]) => cmd(...args),
}));

const { DeveloperPage } = await import("./DeveloperPage");

const run = { id: "1", kind: "dictation", startedAtMs: 1_700_000_000_000, outcome: "succeeded", waitMs: 3200, totalMs: 5200 };
const record = {
  ...run,
  phases: [
    { name: "录音", ms: 2000, waiting: false },
    { name: "智能处理", ms: 3000, waiting: true },
  ],
  promptBreakdown: { promptChars: 1800, hotwordsChars: 300 },
  llm: [{ scope: "smart-text", providerName: "供应商", modelId: "模型甲", durationMs: 2950, usage: { inputTokens: 900 } }],
  analysis: { summary: "最慢的是智能处理", hints: ["模型做了思考"] },
  timeline: [],
};

beforeEach(() => {
  cmd.mockReset();
  cmd.mockImplementation(async (name: string) => {
    if (name === "devTraceRuns") return [run];
    if (name === "devTraceRun") return record;
    if (name === "devTraceStatus") return { tracesDir: "目录", apiUrl: "http://127.0.0.1:5156" };
    return { runs: 0, wait: { samples: 0, p50Ms: 0, p90Ms: 0 }, phases: [], models: [] };
  });
});

afterEach(cleanup);

describe("开发者页面", () => {
  it("选中一次运行后显示结论、各阶段耗时和模型请求", async () => {
    render(<DeveloperPage />);
    fireEvent.click(await screen.findByText(/3\.20 秒/));
    expect(await screen.findByText("最慢的是智能处理")).toBeTruthy();
    expect(screen.getByText("模型做了思考")).toBeTruthy();
    expect(screen.getByText("3.00 秒")).toBeTruthy();
    expect(screen.getByText(/模型甲/)).toBeTruthy();
    expect(cmd).toHaveBeenCalledWith("devTraceRun", { id: "1" });
  });

  it("测试按钮明确告知会产生费用，并把请求发给后端", async () => {
    cmd.mockImplementation(async (name: string) => {
      if (name === "devTraceRuns") return [];
      if (name === "devTraceSmartTest") return { ...record, kind: "smart-test" };
      return { runs: 0, wait: { samples: 0, p50Ms: 0, p90Ms: 0 }, phases: [], models: [] };
    });
    render(<DeveloperPage />);
    fireEvent.click(await screen.findByText("智能处理测试"));
    expect(screen.getByText(/会产生费用/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "开始测试" }));
    await waitFor(() => expect(cmd).toHaveBeenCalledWith("devTraceSmartTest", { request: { fromRun: "latest" } }));
    expect(await screen.findByText("最慢的是智能处理")).toBeTruthy();
  });

  it("本地模型对比把两边的输出和耗时并排列出", async () => {
    cmd.mockImplementation(async (name: string) => {
      if (name === "devTraceRuns") return [];
      if (name === "devLocalCompare") {
        return {
          rawText: "原文",
          appName: "claude",
          scene: "assistant",
          model: "ft-0.8b-Q8_0.gguf",
          local: { output: "本地结果", ms: 180, startupMs: 2700 },
          cloud: { providerId: "default", model: "模型甲", error: "401" },
        };
      }
      return { runs: 0, wait: { samples: 0, p50Ms: 0, p90Ms: 0 }, phases: [], models: [] };
    });
    render(<DeveloperPage />);
    fireEvent.click(await screen.findByText("本地模型对比"));
    fireEvent.click(screen.getByRole("button", { name: "开始对比" }));
    await waitFor(() => expect(cmd).toHaveBeenCalledWith("devLocalCompare", { request: {} }));
    expect(await screen.findByText("本地结果")).toBeTruthy();
    expect(screen.getByText(/向 AI 提需求/)).toBeTruthy();
    expect(screen.getByText("加载模型")).toBeTruthy();
    expect(screen.getByRole("alert").textContent).toBe("401");
  });
});
