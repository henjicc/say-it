import { beforeEach, describe, expect, it, vi } from "vitest";

const cmd = vi.fn();
vi.mock("@/lib/tauri", () => ({
  CMD: new Proxy({}, { get: (_target, key) => String(key) }),
  cmd: (...args: unknown[]) => cmd(...args),
}));
vi.mock("@/store/useDictPrefs", () => ({
  useDictPrefs: {
    getState: () => ({ prefs: { micDeviceId: "" }, dspParams: () => ({}) }),
  },
}));

const { toggleRecord, reprocess } = await import("./lab");
const { useAudioStore } = await import("@/store/useAudioStore");

const idleSnapshot = { recording: false, rawWaveform: [], processedWaveform: [] };

beforeEach(() => {
  cmd.mockReset();
  useAudioStore.setState({ recording: false });
});

describe("音频调校的录音开关", () => {
  it("晚到的旧处理结果不能覆盖新参数，旧请求取消也不显示失败", async () => {
    let oldResolve!: (value: unknown) => void;
    let oldReject!: (error: unknown) => void;
    cmd.mockImplementationOnce(() => new Promise((resolve) => { oldResolve = resolve; }));
    const old = reprocess();
    cmd.mockResolvedValueOnce({ ...idleSnapshot, rawWaveform: [[0, 1]] });
    await reprocess();
    expect(useAudioStore.getState().canPlay).toBe(true);
    oldResolve(idleSnapshot);
    await old;
    expect(useAudioStore.getState().canPlay).toBe(true);

    cmd.mockImplementationOnce(() => new Promise((_resolve, reject) => { oldReject = reject; }));
    const cancelled = reprocess();
    cmd.mockResolvedValueOnce({ ...idleSnapshot, rawWaveform: [[0, 1]] });
    await reprocess();
    oldReject(new Error("音频处理已被新任务替换"));
    await expect(cancelled).resolves.toBeUndefined();
    cmd.mockRejectedValueOnce(new Error("磁盘写入失败"));
    await expect(reprocess()).rejects.toThrow("磁盘写入失败");
  });
  /// 后端已经挡住了重复启动，但按钮此前完全没有在途保护：连点会发出两次
  /// audioLabStart，靠后端兜底，而且两次返回的顺序还会让界面状态来回跳。
  it("请求在途时的重复点击被忽略，只发出一次启动", async () => {
    let release!: (value: unknown) => void;
    cmd.mockImplementation(
      () =>
        new Promise((resolve) => {
          release = resolve;
        }),
    );

    const first = toggleRecord();
    const second = toggleRecord();
    await second;

    expect(cmd).toHaveBeenCalledTimes(1);
    expect(cmd).toHaveBeenCalledWith("audioLabStart", { deviceName: undefined });

    release({ ...idleSnapshot, recording: true });
    await first;
  });

  it("上一次结束后可以再次点击", async () => {
    cmd.mockResolvedValue({ ...idleSnapshot, recording: true });

    await toggleRecord();
    expect(cmd).toHaveBeenCalledTimes(1);

    useAudioStore.setState({ recording: false });
    await toggleRecord();
    expect(cmd).toHaveBeenCalledTimes(2);
  });

  it("失败之后不会把开关卡死", async () => {
    cmd.mockRejectedValueOnce(new Error("麦克风被占用"));
    await toggleRecord();
    expect(useAudioStore.getState().recInfo).toMatch(/录音失败/);

    cmd.mockResolvedValue({ ...idleSnapshot, recording: true });
    await toggleRecord();
    expect(cmd).toHaveBeenCalledTimes(2);
  });
});
