import { create } from "zustand";
import { CMD, EVT, cmd, emitEvent } from "@/lib/tauri";

export interface AccentTheme {
  tone: "dark" | "light";
  accent: string;
  backgroundMode: "followAccent" | "custom";
  background: string;
}

interface ThemeState {
  theme: AccentTheme;
  patch: (partial: Partial<AccentTheme>) => void;
  reset: () => void;
}

const THEME_KEY = "sayItAccentTheme";

export const defaultAccentTheme: AccentTheme = {
  tone: "dark",
  accent: "#5199FF",
  backgroundMode: "followAccent",
  background: "#0A0E16",
};

function normalizeHex(value: string, fallback: string) {
  const raw = value.trim();
  const short = raw.match(/^#?([0-9a-fA-F]{3})$/);
  if (short) {
    return `#${short[1]
      .split("")
      .map((char) => char + char)
      .join("")
      .toUpperCase()}`;
  }

  const full = raw.match(/^#?([0-9a-fA-F]{6})$/);
  return full ? `#${full[1].toUpperCase()}` : fallback;
}

function normalizeTheme(theme: Partial<AccentTheme>): AccentTheme {
  return {
    tone: theme.tone === "light" ? "light" : "dark",
    accent: normalizeHex(theme.accent || "", defaultAccentTheme.accent),
    backgroundMode: theme.backgroundMode === "custom" ? "custom" : "followAccent",
    background: normalizeHex(theme.background || "", defaultAccentTheme.background),
  };
}

function readStored(): AccentTheme {
  try {
    const raw = localStorage.getItem(THEME_KEY);
    if (raw) return normalizeTheme(JSON.parse(raw) as Partial<AccentTheme>);
  } catch {
    /* noop */
  }
  return defaultAccentTheme;
}

function persist(theme: AccentTheme) {
  try {
    localStorage.setItem(THEME_KEY, JSON.stringify(theme));
  } catch {
    /* noop */
  }
}

let themeWriteQueue = Promise.resolve();

function persistToBackend(theme: AccentTheme) {
  themeWriteQueue = themeWriteQueue
    .then(() => cmd(CMD.updateAppSettings, { domain: "theme", value: theme }))
    .then(() => undefined)
    .catch((error) => {
      console.error("保存主题设置失败", error);
    });
}

function broadcast(theme: AccentTheme) {
  void emitEvent(EVT.themeChanged, theme).catch((error) => {
    console.error("同步主题到悬浮窗口失败", error);
  });
}

export function accentContrast(hex: string) {
  const color = normalizeHex(hex, defaultAccentTheme.accent).slice(1);
  const r = parseInt(color.slice(0, 2), 16) / 255;
  const g = parseInt(color.slice(2, 4), 16) / 255;
  const b = parseInt(color.slice(4, 6), 16) / 255;
  const [lr, lg, lb] = [r, g, b].map((channel) =>
    channel <= 0.03928 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4,
  );
  const luminance = 0.2126 * lr + 0.7152 * lg + 0.0722 * lb;
  const blackContrast = (luminance + 0.05) / 0.05;
  const whiteContrast = 1.05 / (luminance + 0.05);
  return blackContrast >= whiteContrast ? "#050505" : "#FFFFFF";
}

function hexToRgb(hex: string) {
  const color = normalizeHex(hex, defaultAccentTheme.accent).slice(1);
  return {
    r: parseInt(color.slice(0, 2), 16),
    g: parseInt(color.slice(2, 4), 16),
    b: parseInt(color.slice(4, 6), 16),
  };
}

function rgbToHex({ r, g, b }: { r: number; g: number; b: number }) {
  return `#${[r, g, b]
    .map((value) => Math.round(Math.max(0, Math.min(255, value))).toString(16).padStart(2, "0"))
    .join("")
    .toUpperCase()}`;
}

function mix(hex: string, target: string, amount: number) {
  const from = hexToRgb(hex);
  const to = hexToRgb(target);
  return rgbToHex({
    r: from.r + (to.r - from.r) * amount,
    g: from.g + (to.g - from.g) * amount,
    b: from.b + (to.b - from.b) * amount,
  });
}

export function accentLight(hex: string) {
  return mix(hex, "#FFFFFF", 0.34);
}

export function accentDark(hex: string) {
  return mix(hex, "#000000", 0.32);
}

export function themeBackground(themeValue: Partial<AccentTheme>) {
  const theme = normalizeTheme(themeValue);
  if (theme.backgroundMode === "custom") return theme.background;
  return mix(theme.accent, theme.tone === "light" ? "#F4F7FB" : "#070A10", 0.96);
}

export function applyThemeToDocument(themeValue: Partial<AccentTheme>, target = document) {
  const theme = normalizeTheme(themeValue);
  const background = themeBackground(theme);
  const root = target.documentElement;
  root.dataset.uiTone = theme.tone;
  root.style.setProperty("--color-accent", theme.accent);
  root.style.setProperty("--color-accent-light", accentLight(theme.accent));
  root.style.setProperty("--color-accent-dark", accentDark(theme.accent));
  root.style.setProperty("--color-accent-contrast", accentContrast(theme.accent));
  const light = theme.tone === "light";
  root.style.setProperty("--color-bg", background);
  root.style.setProperty("--color-bg-sidebar", mix(background, "#000000", light ? 0.04 : 0.18));
  root.style.setProperty("--color-bg-titlebar", mix(background, "#000000", light ? 0.04 : 0.18));
  root.style.setProperty("--color-overlay", mix(background, "#FFFFFF", light ? 0.88 : 0.06));
  root.style.setProperty("--color-fg", light ? "#111827" : "#FFFFFF");
  root.style.setProperty("--color-fg-muted", light ? "rgba(17, 24, 39, 0.68)" : "rgba(255, 255, 255, 0.78)");
  root.style.setProperty("--color-fg-subtle", light ? "rgba(17, 24, 39, 0.42)" : "rgba(255, 255, 255, 0.5)");
  root.style.setProperty("--color-fg-faint", light ? "rgba(17, 24, 39, 0.32)" : "rgba(255, 255, 255, 0.30)");
  root.style.setProperty("--color-surface", light ? "rgba(255, 255, 255, 0.76)" : "rgba(255, 255, 255, 0.035)");
  root.style.setProperty("--color-surface-hover", light ? "rgba(255, 255, 255, 0.92)" : "rgba(255, 255, 255, 0.06)");
  root.style.setProperty("--color-surface-strong", light ? "rgba(255, 255, 255, 0.92)" : "rgba(255, 255, 255, 0.08)");
  root.style.setProperty("--color-line", light ? "rgba(17, 24, 39, 0.1)" : "rgba(255, 255, 255, 0.08)");
  root.style.setProperty("--color-line-strong", light ? "rgba(17, 24, 39, 0.18)" : "rgba(255, 255, 255, 0.16)");
}

export const useThemeStore = create<ThemeState>((set, get) => ({
  theme: readStored(),
  patch: (partial) => {
    const next = normalizeTheme({ ...get().theme, ...partial });
    persist(next);
    set({ theme: next });
    broadcast(next);
    persistToBackend(next);
  },
  reset: () => {
    persist(defaultAccentTheme);
    set({ theme: defaultAccentTheme });
    broadcast(defaultAccentTheme);
    persistToBackend(defaultAccentTheme);
  },
}));

export function hydrateTheme(value: Record<string, unknown>) { const next = normalizeTheme(value as Partial<AccentTheme>); persist(next); useThemeStore.setState({ theme: next }); }
