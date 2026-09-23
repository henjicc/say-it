import { beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.fn();
vi.mock("@/lib/tauri", async (original) => ({
  ...await original<typeof import("@/lib/tauri")>(),
  cmd: (...args: unknown[]) => invoke(...args),
  cmdSilent: (...args: unknown[]) => invoke(...args),
}));

const { useSubtitleStore } = await import("@/store/useSubtitleStore");
const { showSubtitlePreview, hideSubtitlePreview, syncSubtitleIndicator, applySubtitleRuntime } = await import("./controller");

const idle = {
  phase: "idle" as const, originalText: "", translationText: "", obsOutputActive: false,
};

beforeEach(() => {
  invoke.mockReset();
  useSubtitleStore.getState().setRuntime({ running: false, previewActive: false, statusTone: "", statusText: "" });
});

describe("本地字幕预览", () => {
  it("打开预览只走领域命令，不读取旧 WebView 尺寸或启动识别", async () => {
    invoke.mockResolvedValueOnce(undefined).mockResolvedValueOnce({ ...idle, previewActive: true });
    const prefs = useSubtitleStore.getState().prefs;
    await showSubtitlePreview(prefs);
    expect(invoke.mock.calls).toEqual([
      ["show_subtitle_preview", { prefs }], ["get_subtitle_runtime"],
    ]);
    expect(useSubtitleStore.getState().previewActive).toBe(true);
    expect(useSubtitleStore.getState().running).toBe(false);
  });

  it("样式草稿直接交给同一显示通道，关闭仅结束预览", async () => {
    const prefs = { ...useSubtitleStore.getState().prefs, widthPercent: 60 };
    invoke.mockResolvedValueOnce(undefined);
    await syncSubtitleIndicator(prefs);
    invoke.mockResolvedValueOnce(undefined).mockResolvedValueOnce(idle);
    await hideSubtitlePreview();
    expect(invoke.mock.calls).toEqual([
      ["sync_subtitle_presentation", { previewPrefs: prefs }],
      ["hide_subtitle_preview"], ["get_subtitle_runtime"],
    ]);
    expect(useSubtitleStore.getState().previewActive).toBe(false);
  });

  it("字幕窗关闭或正式运行后的快照会复位预览按钮", () => {
    applySubtitleRuntime({ ...idle, previewActive: true });
    expect(useSubtitleStore.getState().previewActive).toBe(true);
    applySubtitleRuntime({ ...idle, previewActive: false });
    expect(useSubtitleStore.getState().previewActive).toBe(false);
    applySubtitleRuntime({ ...idle, phase: "running", previewActive: false });
    expect(useSubtitleStore.getState().running).toBe(true);
  });

  it("预览创建失败不会留下假开启状态，错误对用户可见", async () => {
    invoke.mockRejectedValueOnce(new Error("窗口创建失败"));
    await showSubtitlePreview(useSubtitleStore.getState().prefs);
    expect(useSubtitleStore.getState().previewActive).toBe(false);
    expect(useSubtitleStore.getState().statusTone).toBe("err");
    expect(useSubtitleStore.getState().statusText).toContain("窗口创建失败");
  });
});
