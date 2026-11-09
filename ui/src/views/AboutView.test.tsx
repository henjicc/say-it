import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

const cmd = vi.fn();
vi.mock("@/lib/tauri", () => ({
  CMD: new Proxy({}, { get: (_target, key) => String(key) }),
  cmd: (...args: unknown[]) => cmd(...args),
}));
vi.mock("@tauri-apps/api/app", () => ({
  getName: async () => "说吧！",
  getVersion: async () => "0.5.0-beta.1",
}));
vi.stubEnv("DEV", false);

const { AboutDialog } = await import("./AboutView");
const { useDeveloperStore } = await import("@/store/useDeveloperStore");

afterEach(cleanup);

describe("关于", () => {
  it("连点图标 7 次打开开发者模式，中途提示还差几次", async () => {
    useDeveloperStore.setState({ enabled: false });
    cmd.mockResolvedValue(undefined);
    render(<AboutDialog open onClose={() => {}} />);
    const icon = screen.getByRole("button", { name: /图标/ });

    for (let i = 0; i < 6; i += 1) fireEvent.click(icon);
    expect(screen.getByRole("status").textContent).toBe("再点 1 次开启开发者模式");
    expect(useDeveloperStore.getState().enabled).toBe(false);

    fireEvent.click(icon);
    await waitFor(() => expect(useDeveloperStore.getState().enabled).toBe(true));
    expect(cmd).toHaveBeenCalledWith("updateAppSettings", {
      domain: "diagnostics",
      value: { developerMode: true },
    });
  });
});
