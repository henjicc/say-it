import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const cmd = vi.fn();
vi.mock("@/lib/tauri", () => ({
  CMD: new Proxy({}, { get: (_target, key) => String(key) }),
  cmd: (...args: unknown[]) => cmd(...args),
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
    { providerId: "a", providerName: "甲家", model: "m1" },
    { providerId: "b", providerName: "乙家", model: "m2" },
  ];

  it("没选模型不能开始，选中后把模型和轮数发给后端并显示费用提示", async () => {
    cmd.mockImplementation(async (name: string) => {
      if (name === "devBenchModels") return options;
      if (name === "devBenchRun") return { running: false, models: [row("m1", sample(800, 100))], analysis: { summary: "首字最快的是 m1", hints: [] } };
      return null;
    });
    render(<ModelBenchPanel onFinished={() => {}} />);
    await screen.findByLabelText("甲家 · m1");
    const start = screen.getByRole("button", { name: "开始测速" }) as HTMLButtonElement;
    expect(start.disabled).toBe(true);
    expect(screen.getByText(/会产生费用/)).toBeTruthy();

    fireEvent.click(screen.getByLabelText("甲家 · m1"));
    expect(screen.getByText("同时测速，共 3 次真实请求")).toBeTruthy();
    fireEvent.click(start);

    await waitFor(() => expect(cmd).toHaveBeenCalledWith("devBenchRun", {
      request: { models: [{ providerId: "a", model: "m1" }], rounds: 3 },
    }));
    expect(await screen.findByText("首字最快的是 m1")).toBeTruthy();
  });
});
