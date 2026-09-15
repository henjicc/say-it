import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const cmd = vi.fn();
vi.mock("@/lib/tauri", () => ({
  CMD: new Proxy({}, { get: (_target, key) => String(key) }),
  cmd: (...args: unknown[]) => cmd(...args),
}));

const { ObsOverlayPanel } = await import("./ObsOverlayPanel");

beforeEach(() => {
  cmd.mockReset();
  cmd.mockImplementation((name: string) => {
    if (name === "getObsOverlayStatus") {
      return Promise.resolve({ ready: true, connected: false, url: "", installed: false });
    }
    if (name === "getObsConnectionSettings") {
      return Promise.resolve({ host: "127.0.0.1", port: 4455, hasPassword: true });
    }
    if (name === "connectObs") {
      return Promise.resolve({ connected: true, browserSourceAvailable: true, scenes: [], canvasWidth: 1920, canvasHeight: 1080 });
    }
    return Promise.resolve(undefined);
  });
});

afterEach(cleanup);

describe("ObsOverlayPanel 的密码框", () => {
  /// 这里原本把 16 个 • 直接写进受控 input 的 value，违反「掩码不得进入 input
  /// value / onChange / 保存请求」的铁律：整条防线只靠一个 onFocus 守卫，一旦
  /// 它失效，掩码就会作为真密码被提交上去。改用 SecretInput 后掩码只是 placeholder。
  it("已保存密码时 input value 必须为空，掩码只能出现在 placeholder", async () => {
    render(<ObsOverlayPanel />);
    await waitFor(() => expect(cmd).toHaveBeenCalledWith("getObsConnectionSettings"));

    const password = await screen.findByPlaceholderText(/^•+$/);
    expect((password as HTMLInputElement).value).toBe("");
    expect((password as HTMLInputElement).type).toBe("password");
  });

  it("连接请求在用户没改密码时不带 password 字段", async () => {
    render(<ObsOverlayPanel />);
    await waitFor(() => expect(cmd).toHaveBeenCalledWith("getObsConnectionSettings"));

    screen.getByRole("button", { name: "连接 OBS" }).click();

    await waitFor(() =>
      expect(cmd).toHaveBeenCalledWith("connectObs", {
        request: { host: "127.0.0.1", port: 4455 },
      }),
    );
  });
});
