import { beforeEach, describe, expect, it, vi } from "vitest";

const { cmd, emitEvent } = vi.hoisted(() => ({
  cmd: vi.fn((_command: string, _args?: Record<string, unknown>) => Promise.resolve({})),
  emitEvent: vi.fn((_event: string, _payload?: unknown) => Promise.resolve()),
}));

vi.mock("@/lib/tauri", () => ({
  CMD: { updateAppSettings: "update_app_settings" },
  EVT: { themeChanged: "app-theme-changed" },
  cmd,
  emitEvent,
}));

import {
  applyThemeToDocument,
  defaultAccentTheme,
  useThemeStore,
} from "./useThemeStore";

describe("theme store", () => {
  beforeEach(() => {
    cmd.mockClear();
    emitEvent.mockClear();
    localStorage.clear();
    useThemeStore.setState({ theme: defaultAccentTheme });
  });

  it("applies rapid accent changes immediately and persists them in order", async () => {
    useThemeStore.getState().patch({ accent: "#FF5500" });
    expect(useThemeStore.getState().theme.accent).toBe("#FF5500");

    useThemeStore.getState().patch({ accent: "#12AB34" });
    expect(useThemeStore.getState().theme.accent).toBe("#12AB34");
    expect(emitEvent).toHaveBeenLastCalledWith("app-theme-changed", {
      tone: "dark",
      accent: "#12AB34",
    });

    await vi.waitFor(() => expect(cmd).toHaveBeenCalledTimes(2));
    expect(cmd.mock.calls.map((call) => call[1])).toEqual([
      { domain: "theme", value: { tone: "dark", accent: "#FF5500" } },
      { domain: "theme", value: { tone: "dark", accent: "#12AB34" } },
    ]);
  });

  it("projects light tone and accent tokens into an independent window document", () => {
    applyThemeToDocument({ tone: "light", accent: "#E36B2C" });

    expect(document.documentElement.dataset.uiTone).toBe("light");
    expect(document.documentElement.style.getPropertyValue("--color-accent")).toBe("#E36B2C");
    expect(document.documentElement.style.getPropertyValue("--color-bg")).toBe("#F4F7FB");
  });
});
