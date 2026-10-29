import { beforeEach, describe, expect, it, vi } from "vitest";

const cmd = vi.fn();
vi.mock("@/lib/tauri", () => ({
  CMD: {
    updateAppSettings: "update_app_settings",
    syncSubtitlePresentation: "sync_subtitle_presentation",
    getSubtitleTranslationModel: "get_subtitle_translation_model",
    setSubtitleTranslationModel: "set_subtitle_translation_model",
  },
  cmd: (...args: unknown[]) => cmd(...args),
}));

const { useSubtitleStore, hydrateSubtitlePrefs } = await import("./useSubtitleStore");

beforeEach(() => {
  cmd.mockReset();
  cmd.mockResolvedValue(undefined);
  localStorage.clear();
});

describe("字幕外观参数的夹取", () => {
  /// 这三档的下限就是 0 或负数，而原实现用 `Number(x) || fallback` 统一兜底，
  /// `0 || fallback` 取的是 fallback：滑块拖到头会自己弹回默认值，而且错误值
  /// 还会被写进 localStorage 和后端。OBS / 录屏最常用的恰好就是这三档。
  it("合法的 0 必须保留，不能被当成缺省值", () => {
    useSubtitleStore.getState().patch({
      backgroundOpacity: 0,
      rounded: 0,
      offsetYPercent: 0,
    });

    const prefs = useSubtitleStore.getState().prefs;
    expect(prefs.backgroundOpacity).toBe(0);
    expect(prefs.rounded).toBe(0);
    expect(prefs.offsetYPercent).toBe(0);
  });

  it("负的位置偏移在下限内要原样保留", () => {
    useSubtitleStore.getState().patch({ offsetYPercent: -17 });
    expect(useSubtitleStore.getState().prefs.offsetYPercent).toBe(-17);
  });

  it("超出范围仍然夹到边界", () => {
    useSubtitleStore.getState().patch({ backgroundOpacity: 240, rounded: -5, offsetYPercent: 99 });
    const prefs = useSubtitleStore.getState().prefs;
    expect(prefs.backgroundOpacity).toBe(100);
    expect(prefs.rounded).toBe(0);
    expect(prefs.offsetYPercent).toBe(20);
  });

  it("非数字与未设置的值才回退到默认", () => {
    hydrateSubtitlePrefs({
      backgroundOpacity: "abc",
      rounded: null,
      offsetYPercent: undefined,
      fontSizePercent: Number.NaN,
    });
    const prefs = useSubtitleStore.getState().prefs;
    expect(prefs.backgroundOpacity).toBe(72);
    expect(prefs.rounded).toBe(18);
    expect(prefs.offsetYPercent).toBe(6);
    expect(prefs.fontSizePercent).toBe(2.6);
  });

  it("0 会连同其余参数一起写进后端与本地镜像", () => {
    useSubtitleStore.getState().patch({ backgroundOpacity: 0 });

    expect(cmd).toHaveBeenCalledWith("update_app_settings", {
      domain: "subtitles",
      value: expect.objectContaining({ backgroundOpacity: 0 }),
    });
    expect(JSON.parse(localStorage.getItem("sayItSubtitlePrefs") ?? "{}").backgroundOpacity).toBe(0);
  });
});
