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
  accentContrast,
  applySystemGlassToDocument,
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
      ...defaultAccentTheme,
      accent: "#12AB34",
    });

    await vi.waitFor(() => expect(cmd).toHaveBeenCalledTimes(2));
    expect(cmd.mock.calls.map((call) => call[1])).toEqual([
      { domain: "theme", value: { ...defaultAccentTheme, accent: "#FF5500" } },
      { domain: "theme", value: { ...defaultAccentTheme, accent: "#12AB34" } },
    ]);
  });

  it("projects light tone and accent tokens into an independent window document", () => {
    applyThemeToDocument({ tone: "light", accent: "#E36B2C" });

    expect(document.documentElement.dataset.uiTone).toBe("light");
    expect(document.documentElement.style.getPropertyValue("--color-accent")).toBe("#E36B2C");
    expect(document.documentElement.style.getPropertyValue("--color-accent-contrast")).toBe("#000000");
    expect(document.documentElement.style.getPropertyValue("--theme-bg")).toBe("#F4F4F7");
    expect(document.documentElement.style.getPropertyValue("--system-glass-color")).toBe("#F8F7F8");
  });

  it("uses an explicit background when follow-accent mode is disabled", () => {
    applyThemeToDocument({
      ...defaultAccentTheme,
      backgroundMode: "custom",
      background: "#221A18",
    });

    expect(document.documentElement.style.getPropertyValue("--theme-bg")).toBe("#221A18");
  });

  it("projects global system glass state and clamps its tint", () => {
    applySystemGlassToDocument({ glassEnabled: true, glassTint: 99 });
    expect(document.documentElement.dataset.systemGlass).toBe("true");
    expect(document.documentElement.style.getPropertyValue("--system-glass-tint")).toBe("40%");
  });

  it("chooses readable black or white text from the actual accent luminance", () => {
    expect(accentContrast("#F4D35E")).toBe("#000000");
    expect(accentContrast("#17324D")).toBe("#FFFFFF");
  });
});
