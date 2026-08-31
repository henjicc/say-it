import { StrictMode } from "react";
import { cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { message } from "@tauri-apps/plugin-dialog";
import { useErrorDetailsDialog } from "./useErrorDetailsDialog";

vi.mock("@tauri-apps/plugin-dialog", () => ({ message: vi.fn() }));
const showMessage = vi.mocked(message);

describe("floating orb error details", () => {
  beforeEach(() => showMessage.mockReset());
  afterEach(cleanup);

  it("keeps the complete error after state updates and opens only one dialog", async () => {
    let close!: () => void;
    showMessage.mockImplementationOnce(() => new Promise<string>((resolve) => {
      close = () => resolve("关闭");
    }));
    const hook = renderHook(() => useErrorDetailsDialog(), { wrapper: StrictMode });
    const details = "实时语音识别失败：tls handshake eof\nprovider_realtime_error\n具体原因";
    const pending = hook.result.current(details);
    hook.rerender();
    await hook.result.current("后续状态已改变");
    expect(showMessage).toHaveBeenCalledExactlyOnceWith(details, {
      title: "语音输入出错", kind: "error", buttons: { ok: "关闭" },
    });
    close();
    await pending;
    await hook.result.current("下一次错误");
    expect(showMessage).toHaveBeenCalledTimes(2);
    expect(showMessage.mock.calls[1][0]).toBe("下一次错误");
  });

  it("provides a useful message when the backend supplies no details", async () => {
    const { result } = renderHook(() => useErrorDetailsDialog());
    await result.current("");
    expect(showMessage.mock.calls[0][0]).toBe("语音输入失败，未提供具体错误信息。");
  });

  it("propagates dialog failures and releases the guard for a retry", async () => {
    showMessage.mockRejectedValueOnce(new Error("dialog unavailable"));
    const { result } = renderHook(() => useErrorDetailsDialog());
    await expect(result.current("识别失败")).rejects.toThrow("dialog unavailable");
    await result.current("识别失败");
    expect(showMessage).toHaveBeenCalledTimes(2);
  });
});
