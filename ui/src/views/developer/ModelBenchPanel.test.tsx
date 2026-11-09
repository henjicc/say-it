import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const cmd = vi.fn();
vi.mock("@/lib/tauri", () => ({
  CMD: new Proxy({}, { get: (_target, key) => String(key) }),
  cmd: (...args: unknown[]) => cmd(...args),
}));

const profiles = [{
  id: "a", kind: "llm:custom", displayName: "甲家", authKind: "api-key", capabilities: ["llm"], enabled: true,
  status: { hasApiKey: true }, config: { models: [{ name: "m1", source: "manual" }] },
}];
vi.mock("@/store/useProviderStore", () => ({
  useProviderStore: (selector: (state: { profiles: typeof profiles }) => unknown) => selector({ profiles }),
}));

const { BenchResult, ModelBenchPanel, benchToTsv } = await import("./ModelBenchPanel");

const sample = (firstTokenMs: number, tokensPerSec: number, estimated = false) => ({
  firstTokenMs,
  firstOutputMs: firstTokenMs,
  lastTokenMs: firstTokenMs + 2000,
  outputTokens: 400,
  tokensEstimated: estimated,
  tokensPerSec,
  finished: true,
  error: null,
});

const row = (modelId: string, value: ReturnType<typeof sample>) => ({
  providerId: "p",
  providerName: "供应商",
  modelId,
  samples: [value],
  summary: { ...value, failures: 0 },
});

beforeEach(() => {
  cmd.mockReset();
});

afterEach(cleanup);

describe("模型测速结果", () => {
  it("跑完后分别标出首字最快和生成最快的模型，估算值带约等号", () => {
    const models = [row("首字快", sample(800, 100)), row("生成快", sample(2000, 500, true))];
    render(<BenchResult state={{ running: false, models }} />);
    // 同一个数字在每轮明细里还会出现一次，第一个是对比表里的。
    expect(screen.getAllByText("800 毫秒")[0].className).toContain("font-semibold");
    expect(screen.getAllByText("2.00 秒")[0].className).not.toContain("font-semibold");
    expect(screen.getAllByText("≈500")[0].className).toContain("font-semibold");
    expect(screen.getAllByText("100")[0].className).not.toContain("font-semibold");
  });

  it("测速进行中显示当前这一轮的数字和进度，不标最优", () => {
    const running = { ...sample(900, 200), finished: false };
    const pending = { ...sample(900, 200), firstTokenMs: null, firstOutputMs: null, lastTokenMs: 0, tokensPerSec: null, outputTokens: null, finished: false };
    const model = {
      providerId: "p",
      providerName: "供应商",
      modelId: "甲",
      samples: [running, pending, pending],
      summary: { firstTokenMs: null, firstOutputMs: null, lastTokenMs: null, tokensPerSec: null, outputTokens: null, tokensEstimated: false, failures: 0 },
    };
    render(<BenchResult state={{ running: true, models: [model, row("乙", sample(500, 300))] }} />);
    expect(screen.getByText("供应商 · 第 1/3 轮")).toBeTruthy();
    expect(screen.getAllByText("900 毫秒")[0].className).not.toContain("font-semibold");
  });

  it("失败的模型显示失败轮数和原因", () => {
    const failed = { ...sample(0, 0), error: "401 未授权" };
    const model = { ...row("坏模型", failed), summary: { firstTokenMs: null, firstOutputMs: null, lastTokenMs: null, tokensPerSec: null, outputTokens: null, tokensEstimated: false, failures: 1 } };
    render(<BenchResult state={{ running: false, models: [model] }} />);
    expect(screen.getByRole("alert").textContent).toContain("1/1 轮失败：401 未授权");
  });

  it("复制的表格可以直接贴进表格软件，缺失的值留空", () => {
    const failed = { ...row("坏模型", sample(0, 0)), summary: { firstTokenMs: null, firstOutputMs: null, lastTokenMs: null, tokensPerSec: null, outputTokens: null, tokensEstimated: false, failures: 3 } };
    const lines = benchToTsv([row("甲", sample(800, 123.4)), failed]).split("\n");
    expect(lines[1]).toBe("供应商\t甲\t800\t800\t123\t2800\t400\t0");
    expect(lines[2]).toBe("供应商\t坏模型\t\t\t\t\t\t3");
  });
});

describe("模型测速面板", () => {
  const options = [
    { providerId: "a", providerName: "甲家", model: "m1", current: false },
    { providerId: "b", providerName: "乙家", model: "qwen3-asr-flash", current: false },
    { providerId: "a", providerName: "甲家", model: "m3", current: true },
  ];

  it("只列用途里能选的大模型和当前模型，默认勾上当前模型，把选中的模型和轮数发给后端", async () => {
    cmd.mockImplementation(async (name: string) => {
      if (name === "devBenchModels") return options;
      if (name === "devBenchRun") return { running: false, models: [row("m1", sample(800, 100))], analysis: { summary: "首字最快的是 m1", hints: [] } };
      return null;
    });
    render(<ModelBenchPanel onFinished={() => {}} />);
    const current = await screen.findByLabelText("甲家 · m3");
    expect((current as HTMLInputElement).checked).toBe(true);
    expect(screen.queryByLabelText("乙家 · qwen3-asr-flash")).toBeNull();
    expect(screen.getByText(/会产生费用/)).toBeTruthy();
    expect(screen.getByText("1 个模型同时测速，共 3 次真实请求")).toBeTruthy();

    fireEvent.click(screen.getByLabelText("甲家 · m1"));
    expect(screen.getByText("2 个模型同时测速，共 6 次真实请求")).toBeTruthy();
    fireEvent.click(screen.getByRole("radio", { name: "关闭" }));
    fireEvent.click(screen.getByRole("button", { name: "开始测速" }));

    await waitFor(() => expect(cmd).toHaveBeenCalledWith("devBenchRun", {
      request: { models: [{ providerId: "a", model: "m1" }, { providerId: "a", model: "m3" }], rounds: 3, reasoningEffort: "zero" },
    }));
    expect(await screen.findByText("首字最快的是 m1")).toBeTruthy();
  });
});
