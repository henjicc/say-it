import { beforeEach, describe, expect, it, vi } from "vitest";

const cmd = vi.fn();
vi.mock("@/lib/tauri", () => ({
  CMD: new Proxy({}, { get: (_target, key) => String(key) }),
  cmd: (...args: unknown[]) => cmd(...args),
  cmdSilent: (...args: unknown[]) => cmd(...args),
}));

const {
  configureSubtitleHotkeys,
  handleForwardedSubtitleKeydown,
  handleForwardedSubtitleKeyup,
  loadSubtitleShortcut,
} = await import("./hotkeys");

const toggle = vi.fn();
configureSubtitleHotkeys({ setStatus: () => {}, toggle });

/** 让后端返回一份字幕快捷键配置，并说明它是否已被全局接管。 */
async function loadShortcut(globalRegistered: boolean) {
  cmd.mockResolvedValue({ key_code: "F2", global_registered: globalRegistered });
  await loadSubtitleShortcut();
}

beforeEach(() => {
  cmd.mockReset();
  toggle.mockReset();
  handleForwardedSubtitleKeyup("F2");
});

describe("字幕热键的焦点兜底", () => {
  /// Windows 的低级钩子对非锁定键只上报不吞键，按键会继续传给前台窗口。主窗口
  /// 聚焦时，钩子已经 toggle 过一次，前端的焦点兜底拿到同一次按键又 toggle 一次
  /// ——字幕开了又立刻关，用户按一次什么都没发生。
  it("全局路径已接管时不再重复触发", async () => {
    await loadShortcut(true);

    handleForwardedSubtitleKeydown({ code: "F2" });

    expect(toggle).not.toHaveBeenCalled();
  });

  /// 全局注册没生效时，兜底是唯一能用的路径，必须照常工作。
  it("全局注册未生效时仍然兜底", async () => {
    await loadShortcut(false);

    handleForwardedSubtitleKeydown({ code: "F2" });

    expect(toggle).toHaveBeenCalledTimes(1);
  });

  it("兜底路径本身仍然只响应一次按下", async () => {
    await loadShortcut(false);

    handleForwardedSubtitleKeydown({ code: "F2" });
    handleForwardedSubtitleKeydown({ code: "F2" });

    expect(toggle).toHaveBeenCalledTimes(1);
  });

  it("修饰键不匹配时不触发", async () => {
    await loadShortcut(false);

    handleForwardedSubtitleKeydown({ code: "F2", ctrlKey: true });

    expect(toggle).not.toHaveBeenCalled();
  });
});
