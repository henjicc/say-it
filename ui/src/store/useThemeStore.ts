import { create } from "zustand";

export interface AccentTheme {
  accent: string;
  accentLight: string;
  accentDark: string;
}

interface ThemeState {
  theme: AccentTheme;
  patch: (partial: Partial<AccentTheme>) => void;
  reset: () => void;
}

const THEME_KEY = "sayItAccentTheme";

export const defaultAccentTheme: AccentTheme = {
  accent: "#5199FF",
  accentLight: "#8EC1FF",
  accentDark: "#1C6FEA",
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
    accent: normalizeHex(theme.accent || "", defaultAccentTheme.accent),
    accentLight: normalizeHex(theme.accentLight || "", defaultAccentTheme.accentLight),
    accentDark: normalizeHex(theme.accentDark || "", defaultAccentTheme.accentDark),
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
  return luminance > 0.58 ? "#050505" : "#FFFFFF";
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
