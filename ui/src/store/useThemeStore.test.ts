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
  accentControlColors,
  accentContrast,
  applySystemGlassToDocument,
  applyThemeToDocument,
  backgroundTone,
  contrastRatio,
  defaultAccentTheme,
  hydrateTheme,
  useThemeStore,
} from "./useThemeStore";

describe("theme store", () => {
  beforeEach(() => {
    cmd.mockClear();
    emitEvent.mockClear();
    localStorage.clear();
    useThemeStore.setState({ theme: defaultAccentTheme });
  });

  it("uses the product default accent and base background", () => {
    expect(defaultAccentTheme).toEqual({
      tone: "dark",
      accent: "#2767EA",
      backgroundMode: "custom",
      background: "#0F0F0F",
    });
    applyThemeToDocument(defaultAccentTheme);
    expect(document.documentElement.style.getPropertyValue("--theme-bg")).toBe("#0F0F0F");
    expect(document.documentElement.style.getPropertyValue("--color-accent")).toBe("#2767EA");
  });

  it("switches a dark custom background to the light default when the tone turns light", () => {
    expect(backgroundTone("#0F0F0F")).toBe("dark");
    useThemeStore.getState().patch({ tone: "light" });
    expect(useThemeStore.getState().theme.background).toBe("#F4F4F5");
    applyThemeToDocument(useThemeStore.getState().theme);
    expect(document.documentElement.style.getPropertyValue("--theme-bg")).toBe("#F4F4F5");
  });

  it("writes a stored theme back once its background had to be replaced", async () => {
    hydrateTheme({ tone: "light", accent: "#2767EA", backgroundMode: "custom", background: "#0F0F0F" });
    await vi.waitFor(() => expect(cmd).toHaveBeenCalledWith("update_app_settings", {
      domain: "theme",
      value: { tone: "light", accent: "#2767EA", backgroundMode: "custom", background: "#F4F4F5" },
    }));
  });

  it("keeps readable and accent text at AA on every text layer in both tones", () => {
    for (const tone of ["dark", "light"] as const) {
      applyThemeToDocument({ ...defaultAccentTheme, tone });
      const style = document.documentElement.style;
      const layers = ["--theme-bg-sidebar", "--theme-bg", "--theme-surface", "--theme-overlay"]
        .map((name) => style.getPropertyValue(name));
      for (const text of ["--color-fg-subtle", "--color-accent-text"]) {
        for (const layer of layers) {
          expect(contrastRatio(layer, style.getPropertyValue(text))).toBeGreaterThanOrEqual(4.5);
        }
      }
    }
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
    expect(document.documentElement.style.getPropertyValue("--color-accent-control-contrast")).toBe("#FFFFFF");
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

    useThemeStore.getState().patch({ backgroundMode: "custom", background: "#282C34" });
    useThemeStore.getState().patch({ accent: "#E36B2C" });
    expect(useThemeStore.getState().theme.background).toBe("#282C34");
  });

  it("projects global system glass state and clamps its tint", () => {
    applySystemGlassToDocument({ glassEnabled: true, glassTint: 99 });
    expect(document.documentElement.dataset.systemGlass).toBe("true");
    expect(document.documentElement.style.getPropertyValue("--system-glass-tint")).toBe("40%");
  });

  it("keeps raw-color contrast available for non-control projections", () => {
    expect(accentContrast("#F4D35E")).toBe("#000000");
    expect(accentContrast("#17324D")).toBe("#FFFFFF");
  });

  it("uses white text for medium blue controls and minimally darkens the fill to AA", () => {
    const blue = accentControlColors("#5199FF");
    expect(blue.foreground).toBe("#FFFFFF");
    expect(contrastRatio(blue.background, blue.foreground)).toBeGreaterThanOrEqual(4.5);

    const yellow = accentControlColors("#F4D35E");
    expect(yellow.foreground).toBe("#000000");
    expect(contrastRatio(yellow.background, yellow.foreground)).toBeGreaterThanOrEqual(4.5);
  });
});
