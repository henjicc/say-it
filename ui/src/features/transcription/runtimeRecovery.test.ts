import { beforeEach, describe, expect, it, vi } from "vitest";

const cmd = vi.fn();
vi.mock("@/lib/tauri", () => ({
  CMD: new Proxy({}, { get: (_target, key) => String(key) }),
  EVT: new Proxy({}, { get: (_target, key) => String(key) }),
  cmd: (...args: unknown[]) => cmd(...args),
  cmdSilent: (...args: unknown[]) => cmd(...args),
  on: async () => () => undefined,
}));

const { loadTranscriptionRuntime } = await import("./controller");
const { useTranscriptionStore } = await import("@/store/useTranscriptionStore");

const initial = useTranscriptionStore.getState();

type Job = {
  jobId: string;
  kind: string;
  stage: string;
  active: boolean;
  payload: Record<string, unknown>;
};

const serve = (jobs: Job[]) => {
  cmd.mockImplementation(async (name: string) =>
    name === "getTranscriptionRuntime" ? jobs : undefined,
  );
};

beforeEach(() => {
  cmd.mockReset();
  useTranscriptionStore.setState({
    stage: initial.stage,
    jobId: "",
    statusText: "",
    alignStage: initial.alignStage,
    alignJobId: "",
    alignStatusText: "",
  });
});

describe("主窗口重建后的任务恢复", () => {
  it("按后端登记顺序恢复最新运行任务，不按随机 ID 排序", async () => {
    serve([
      { jobId: "z-old", kind: "transcribe", stage: "polling", active: true, payload: {} },
      { jobId: "a-new", kind: "transcribe", stage: "polling", active: true, payload: {} },
    ]);
    await loadTranscriptionRuntime();
    expect(useTranscriptionStore.getState().jobId).toBe("a-new");
  });

  it("仅有已结束任务时恢复最新结果，运行任务仍优先于已结束任务", async () => {
    serve([
      { jobId: "z-old", kind: "transcribe", stage: "completed", active: false, payload: {} },
      { jobId: "a-new", kind: "transcribe", stage: "completed", active: false, payload: { result: { transcripts: [{ text: "最新结果", sentences: [] }] } } },
    ]);
    await loadTranscriptionRuntime();
    expect(useTranscriptionStore.getState().jobId).toBe("a-new");
    expect(useTranscriptionStore.getState().result?.transcripts[0]?.text).toBe("最新结果");
    useTranscriptionStore.setState({ jobId: "" });
    serve([
      { jobId: "older-running", kind: "transcribe", stage: "polling", active: true, payload: {} },
      { jobId: "newer-done", kind: "transcribe", stage: "completed", active: false, payload: {} },
    ]);
    await loadTranscriptionRuntime();
    expect(useTranscriptionStore.getState().jobId).toBe("older-running");
  });

  /// 归属信息以前只存在前端内存（store.alignJobId），`destroy_main_window` 之后就没了。
  /// 恢复时最近的一项无论属于谁都走普通转写分支，于是文稿对齐的进度/结果被整个投影到
  /// 「字幕转写」页上——用户在那页看到一条自己没发起过的识别任务。
  it("文稿对齐的任务不会被投影到字幕转写页", async () => {
    serve([{ jobId: "align-1", kind: "align", stage: "polling", active: true, payload: { pollCount: 2 } }]);

    await loadTranscriptionRuntime();

    const store = useTranscriptionStore.getState();
    expect(store.stage).toBe("idle");
    expect(store.jobId).toBe("");
    expect(store.alignStage).toBe("recognizing");
    expect(store.alignJobId).toBe("align-1");
  });

  it("普通转写的任务仍然照常恢复", async () => {
    serve([{ jobId: "job-1", kind: "transcribe", stage: "polling", active: true, payload: { pollCount: 1 } }]);

    await loadTranscriptionRuntime();

    const store = useTranscriptionStore.getState();
    expect(store.stage).toBe("recognizing");
    expect(store.jobId).toBe("job-1");
    expect(store.alignStage).toBe("idle");
  });

  /// 模型对比与 file 模式听写走的是同一条 transcription_start，但各有自己的界面。
  it("模型对比与 file 模式听写的任务都不投影到这两个页面", async () => {
    serve([
      { jobId: "cmp-1", kind: "compare", stage: "polling", active: true, payload: {} },
      { jobId: "dict-1", kind: "dictation", stage: "polling", active: true, payload: {} },
    ]);

    await loadTranscriptionRuntime();

    const store = useTranscriptionStore.getState();
    expect(store.stage).toBe("idle");
    expect(store.alignStage).toBe("idle");
  });

  it("两个页面各自恢复自己最近的一项", async () => {
    serve([
      { jobId: "job-1", kind: "transcribe", stage: "polling", active: true, payload: {} },
      { jobId: "align-1", kind: "align", stage: "polling", active: true, payload: {} },
    ]);

    await loadTranscriptionRuntime();

    const store = useTranscriptionStore.getState();
    expect(store.jobId).toBe("job-1");
    expect(store.alignJobId).toBe("align-1");
  });
});
