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

type Tone = AccentTheme["tone"];

/** 每种色调的默认背景基色：亮色用清亮冷白，暗色用通透深灰蓝；自定义背景不适合当前色调时也回落到这里。 */
const TONE_BACKGROUND: Record<Tone, string> = {
  light: "#F6F8FB",
  dark: "#111318",
};

export const defaultAccentTheme: AccentTheme = {
  tone: "light",
  accent: "#2563EB",
  backgroundMode: "custom",
  background: TONE_BACKGROUND.light,
};

/** 只随色调变化、不随用户颜色变化的令牌。--color-fg-subtle 是需要阅读的文字的下限。 */
const TONE_PALETTE: Record<Tone, Record<string, string>> = {
  dark: {
    "--color-fg": "#F1F5F9",
    "--color-fg-muted": "#B4BDCC",
    "--color-fg-subtle": "#94A3B8",
    "--color-fg-faint": "#64748B",
    "--color-ok": "#34D399",
    "--color-err": "#F87171",
    "--color-warn": "#FBBF24",
    "--color-line": "rgba(255, 255, 255, 0.08)",
    "--color-line-strong": "rgba(255, 255, 255, 0.15)",
    "--color-scrim": "rgba(0, 0, 0, 0.65)",
    "--shadow-sm": "0 1px 2px rgba(0, 0, 0, 0.35)",
    "--shadow-raise": "0 4px 16px -8px rgba(0, 0, 0, 0.4), inset 0 1px 0 rgba(255, 255, 255, 0.035)",
    "--shadow-popover": "0 16px 40px -8px rgba(0, 0, 0, 0.58)",
  },
  light: {
    "--color-fg": "#0F172A",
    "--color-fg-muted": "#475569",
    "--color-fg-subtle": "#526074",
    "--color-fg-faint": "#94A3B8",
    "--color-ok": "#059669",
    "--color-err": "#DC2626",
    "--color-warn": "#D97706",
    "--color-line": "rgba(15, 23, 42, 0.08)",
    "--color-line-strong": "rgba(15, 23, 42, 0.14)",
    "--color-scrim": "rgba(15, 23, 42, 0.28)",
    "--shadow-sm": "0 1px 2px rgba(15, 23, 42, 0.04)",
    // 亮色分组面用纯白浮卡 + 柔和轻影抬起，避免厚重灰框。
    "--shadow-raise": "0 1px 2px rgba(15, 23, 42, 0.03), 0 6px 20px -10px rgba(15, 23, 42, 0.08)",
    "--shadow-popover": "0 12px 32px -8px rgba(15, 23, 42, 0.14), 0 2px 6px rgba(15, 23, 42, 0.06)",
  },
};

/** 强调色柔和底的浓度（%）：soft 用于悬停、多选行与大面积提示，strong 用于标签与时间轴高亮。 */
const ACCENT_SOFT = 14;
const ACCENT_SOFT_STRONG = 20;

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
  const tone: Tone = theme.tone === "dark" ? "dark" : "light";
  const background = normalizeHex(theme.background || "", TONE_BACKGROUND[tone]);
  return {
    tone,
    accent: normalizeHex(theme.accent || "", defaultAccentTheme.accent),
    backgroundMode: theme.backgroundMode === "custom" ? "custom" : "followAccent",
    // 背景基色只属于一种色调：深色背景放进亮色调（或反过来）会让整套层级和文字失效，
    // 所以明暗与当前色调相反的背景换成该色调的默认背景。
    background: backgroundTone(background) === tone ? background : TONE_BACKGROUND[tone],
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
  return blackContrast >= whiteContrast ? "#000000" : "#FFFFFF";
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

function relativeLuminance(hex: string) {
  const { r, g, b } = hexToRgb(hex);
  const [lr, lg, lb] = [r, g, b].map((value) => {
    const channel = value / 255;
    return channel <= 0.03928 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * lr + 0.7152 * lg + 0.0722 * lb;
}

export function contrastRatio(first: string, second: string) {
  const lighter = Math.max(relativeLuminance(first), relativeLuminance(second));
  const darker = Math.min(relativeLuminance(first), relativeLuminance(second));
  return (lighter + 0.05) / (darker + 0.05);
}

/** 把 color 朝 toward 做最小幅度的混合，直到与 against 的对比度达到 AA。 */
function ensureContrast(color: string, against: string, toward: "#000000" | "#FFFFFF") {
  if (contrastRatio(color, against) >= 4.5) return color;
  let low = 0;
  let high = 1;
  for (let index = 0; index < 18; index += 1) {
    const amount = (low + high) / 2;
    if (contrastRatio(mix(color, toward, amount), against) >= 4.5) high = amount;
    else low = amount;
  }
  return mix(color, toward, high);
}

export function accentControlColors(accent: string) {
  // 文本型主按钮遵循桌面应用的视觉语义：中深色、饱和色优先白字，只有真正的
  // 浅色强调色才使用黑字。若原色达不到 AA，对填充色做最小幅度明暗修正。
  const foreground = (relativeLuminance(accent) >= 0.48 ? "#000000" : "#FFFFFF") as
    "#000000" | "#FFFFFF";
  const away = foreground === "#FFFFFF" ? "#000000" : "#FFFFFF";
  const background = ensureContrast(accent, foreground, away);
  const hover = ensureContrast(mix(background, "#FFFFFF", 0.06), foreground, away);
  const active = ensureContrast(mix(background, "#000000", 0.1), foreground, away);
  return { background, foreground, hover, active };
}

export function accentLight(hex: string) {
  return mix(hex, "#FFFFFF", 0.34);
}

interface ThemeLayers {
  frame: string;
  background: string;
  overlay: string;
  surface: string;
  surfaceHover: string;
  surfaceStrong: string;
}

/**
 * 明度层级都从背景色推导：外框比内容层更沉，分组面、浮层依次抬起；悬停与按下
 * 在暗色里更亮、在亮色里更深。用户换背景色时层级关系保持不变。
 */
function themeLayers(background: string, tone: Tone): ThemeLayers {
  if (tone === "light") {
    return {
      frame: mix(background, "#0F172A", 0.032),
      background,
      overlay: mix(background, "#FFFFFF", 1),
      surface: mix(background, "#FFFFFF", 0.98),
      surfaceHover: mix(background, "#0F172A", 0.035),
      surfaceStrong: mix(background, "#0F172A", 0.075),
    };
  }
  return {
    frame: mix(background, "#000000", 0.24),
    background,
    // 浮层要明显亮过分组面，叠在分组上的下拉和弹层才分得出来。
    overlay: mix(background, "#FFFFFF", 0.1),
    surface: mix(background, "#FFFFFF", 0.055),
    surfaceHover: mix(background, "#FFFFFF", 0.085),
    surfaceStrong: mix(background, "#FFFFFF", 0.13),
  };
}

/** 背景属于哪种色调：黑字比白字更清楚的是浅色背景，属于亮色调。 */
export function backgroundTone(background: string): Tone {
  const color = normalizeHex(background, TONE_BACKGROUND.dark);
  return contrastRatio(color, "#000000") >= contrastRatio(color, "#FFFFFF") ? "light" : "dark";
}

/**
 * 新填的背景基色是否可用：与色调同侧，且最浅一级可读文字在它上面达到 AA。
 * 只用于拦截新输入；已保存的值由 normalizeTheme 按明暗侧纠正，不因这条更严的线被改掉。
 */
export function backgroundSupportsText(background: string, tone: Tone) {
  const color = normalizeHex(background, TONE_BACKGROUND[tone]);
  return backgroundTone(color) === tone
    && contrastRatio(color, TONE_PALETTE[tone]["--color-fg-subtle"]) >= 4.5;
}

/**
 * 强调色作为文字与图标时的颜色（链接、关键状态、标签）。要在各层以及叠了柔和强调底
 * 的各层上都达到 AA：暗色调向白提亮、亮色调向黑压暗，只做最小幅度修正。
 */
function accentTextColor(accent: string, tone: Tone, layers: ThemeLayers) {
  const toward = tone === "light" ? "#000000" : "#FFFFFF";
  const plain = [layers.frame, layers.background, layers.surface, layers.overlay];
  return [...plain, ...plain.map((layer) => mix(layer, accent, ACCENT_SOFT_STRONG / 100))]
    .reduce((color, against) => ensureContrast(color, against, toward), accent);
}

export function themeBackground(themeValue: Partial<AccentTheme>) {
  const theme = normalizeTheme(themeValue);
  if (theme.backgroundMode === "custom") return theme.background;
  return mix(theme.accent, theme.tone === "light" ? "#F4F7FB" : "#070A10", 0.98);
}

export function applyThemeToDocument(themeValue: Partial<AccentTheme>, target = document) {
  const theme = normalizeTheme(themeValue);
  const light = theme.tone === "light";
  const background = themeBackground(theme);
  const layers = themeLayers(background, theme.tone);
  const root = target.documentElement;
  const set = (name: string, value: string) => root.style.setProperty(name, value);
  root.dataset.uiTone = theme.tone;
  // 原生部件（文本框拖拽角、日期选择等）跟随色调，否则深色界面里会画成浅色。
  root.style.colorScheme = light ? "light" : "dark";
  for (const [name, value] of Object.entries(TONE_PALETTE[theme.tone])) set(name, value);

  set("--color-accent", theme.accent);
  set("--color-accent-light", accentLight(theme.accent));
  set("--color-accent-contrast", accentContrast(theme.accent));
  set("--color-accent-light-contrast", accentContrast(accentLight(theme.accent)));
  const control = accentControlColors(theme.accent);
  set("--color-accent-control", control.background);
  set("--color-accent-control-hover", control.hover);
  set("--color-accent-control-active", control.active);
  set("--color-accent-control-contrast", control.foreground);
  set("--color-accent-text", accentTextColor(theme.accent, theme.tone, layers));
  set("--accent-soft", `color-mix(in srgb, var(--color-accent) ${ACCENT_SOFT}%, transparent)`);
  set("--accent-soft-strong", `color-mix(in srgb, var(--color-accent) ${ACCENT_SOFT_STRONG}%, transparent)`);

  set("--theme-bg", layers.background);
  set("--theme-bg-sidebar", layers.frame);
  set("--theme-bg-titlebar", layers.frame);
  set("--theme-overlay", layers.overlay);
  set("--theme-surface", layers.surface);
  set("--theme-surface-hover", layers.surfaceHover);
  set("--theme-surface-strong", layers.surfaceStrong);
  const glassBase = theme.backgroundMode === "custom"
    ? background
    : light
      ? "#F8FAFC"
      : "#05070B";
  set("--system-glass-color", mix(theme.accent, glassBase, light ? 0.98 : 0.97));
}

export function applySystemGlassToDocument(
  settings: { glassEnabled?: boolean; glassTint?: number },
  target = document,
) {
  const root = target.documentElement;
  root.dataset.systemGlass = settings.glassEnabled === true ? "true" : "false";
  const tint = Math.max(0, Math.min(40, Math.round(Number(settings.glassTint) || 0)));
  root.style.setProperty("--system-glass-tint", `${tint}%`);
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

export function hydrateTheme(value: Record<string, unknown>) {
  const next = normalizeTheme(value as Partial<AccentTheme>);
  persist(next);
  useThemeStore.setState({ theme: next });
  // 早期版本可能存下了不适合当前色调的背景；原生窗口的材质色直接读后端里的主题，
  // 所以被替换后要回写一次，让两边一致。
  if (next.background !== value.background) persistToBackend(next);
}
