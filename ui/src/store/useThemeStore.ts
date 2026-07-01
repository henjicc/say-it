import { create } from "zustand";

export interface AccentTheme {
  tone: "dark" | "light";
  accent: string;
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

export const useThemeStore = create<ThemeState>((set, get) => ({
  theme: readStored(),
  patch: (partial) => {
    const next = normalizeTheme({ ...get().theme, ...partial });
    persist(next);
    set({ theme: next });
  },
  reset: () => {
    persist(defaultAccentTheme);
    set({ theme: defaultAccentTheme });
  },
}));
