import { create } from "zustand";
import { CMD, cmd } from "@/lib/tauri";
import {
  DEFAULT_REALTIME_ASR_MODEL,
  isSupportedRealtimeModel,
} from "@/features/asr/modelOptions";
import { TRANSLATION_MODEL_NONE, normalizeTranslationModel } from "@/features/translation/models";
import { defaultSubtitleFontFamily } from "@/lib/platform";
import {
  DEFAULT_TRANSLATION_SOURCE_LANG,
  DEFAULT_TRANSLATION_TARGET_LANG,
} from "@/features/translation/languages";

/**
 * 声音来源用一个字符串 id 表达："mic:default" / "system:default" 是最上面的
 * 默认麦克风/系统音频；"mic:<设备名>" / "system:<设备名>" 指向某个具体的输入/
 * 播放设备（系统音频这边是把播放设备当输入设备做 loopback 采集）。
 */
export type SubtitleSource = string;
export type SubtitleSourceKind = "mic" | "system";
export const DEFAULT_SUBTITLE_SOURCE: SubtitleSource = "mic:default";

export function buildSubtitleSource(kind: SubtitleSourceKind, deviceName?: string): SubtitleSource {
  return deviceName ? `${kind}:${deviceName}` : `${kind}:default`;
}

export function parseSubtitleSource(source: SubtitleSource): { kind: SubtitleSourceKind; deviceName?: string } {
  const [kind, ...rest] = source.split(":");
  const deviceName = rest.join(":");
  return {
    kind: kind === "system" ? "system" : "mic",
    deviceName: deviceName && deviceName !== "default" ? deviceName : undefined,
  };
}

export type SubtitleAnchor = "top" | "center" | "bottom";
export type SubtitleMode = "scroll" | "replace";
export type SubtitleAnimationEasing = "ease-out" | "ease-in-out" | "linear" | "ease-in";
/** 双语时的显示范围：bilingual = 原文+译文都显示；translationOnly = 只显示译文。 */
export type SubtitleTranslationLayout = "bilingual" | "translationOnly";
/** 双语时的上下顺序。 */
export type SubtitleTranslationOrder = "sourceFirst" | "translationFirst";

export interface SubtitlePrefs {
  source: SubtitleSource;
  asrModel: string;
  mode: SubtitleMode;
  fontFamily: string;
  /** 字号，屏幕高度的百分比 */
  fontSizePercent: number;
  lineCount: number;
  /** 字幕框最大宽度，屏幕宽度的百分比 */
  widthPercent: number;
  anchor: SubtitleAnchor;
  /** 相对锚点的位置偏移，屏幕高度的百分比 */
  offsetYPercent: number;
  textColor: string;
  backgroundColor: string;
  backgroundOpacity: number;
  rounded: number;
  /** 位移动画：内容更新时的左右平移（单句替换）/上下滚动（滚动累积）。 */
  motionEnabled: boolean;
  motionDurationMs: number;
  motionEasing: SubtitleAnimationEasing;
  /** 淡入动画：新增文字出现时的不透明度过渡。 */
  fadeEnabled: boolean;
  fadeDurationMs: number;
  fadeEasing: SubtitleAnimationEasing;
  /** 字幕翻译所用的 Qwen-MT 模型；空串表示关闭翻译。 */
  translationModel: string;
  translationSourceLang: string;
  translationTargetLang: string;
  translationLayout: SubtitleTranslationLayout;
  translationOrder: SubtitleTranslationOrder;
  /** 输出到 OBS：关闭后即使 OBS 字幕源在线，字幕也回到桌面悬浮窗显示（不断开 OBS 连接）。 */
  obsOutputEnabled: boolean;
}

type Tone = "" | "ok" | "err";

interface SubtitleState {
  prefs: SubtitlePrefs;
  running: boolean;
  previewActive: boolean;
  statusText: string;
  statusTone: Tone;
  latestText: string;
  capturing: boolean;
  shortcutLabel: string;
  /** 字幕当前是否实际输出到 OBS（连接就绪且"输出到 OBS"开关打开），用于界面隐藏不适用的样式项。 */
  obsOutputActive: boolean;
  patch: (partial: Partial<SubtitlePrefs>) => void;
  loadTranslationModel: () => Promise<void>;
  setTranslationModel: (model: string) => Promise<void>;
  setRuntime: (
    partial: Partial<
      Pick<
        SubtitleState,
        "running" | "previewActive" | "statusText" | "statusTone" | "latestText" | "capturing" | "shortcutLabel" | "obsOutputActive"
      >
    >,
  ) => void;
}

const SUBTITLE_PREFS_KEY = "sayItSubtitlePrefs";

const defaults = (): SubtitlePrefs => ({
  source: DEFAULT_SUBTITLE_SOURCE,
  asrModel: DEFAULT_REALTIME_ASR_MODEL,
  mode: "replace",
  fontFamily: defaultSubtitleFontFamily,
  fontSizePercent: 2.6,
  lineCount: 1,
  widthPercent: 46,
  anchor: "bottom",
  offsetYPercent: 6,
  textColor: "#ffffff",
  backgroundColor: "#05070a",
  backgroundOpacity: 72,
  rounded: 18,
  motionEnabled: false,
  motionDurationMs: 120,
  motionEasing: "ease-out",
  fadeEnabled: false,
  fadeDurationMs: 180,
  fadeEasing: "ease-out",
  translationModel: TRANSLATION_MODEL_NONE,
  translationSourceLang: DEFAULT_TRANSLATION_SOURCE_LANG,
  translationTargetLang: DEFAULT_TRANSLATION_TARGET_LANG,
  translationLayout: "bilingual",
  translationOrder: "translationFirst",
  obsOutputEnabled: false,
});

const SUBTITLE_ANIMATION_EASINGS: SubtitleAnimationEasing[] = ["ease-out", "ease-in-out", "linear", "ease-in"];

function clampEasing(value: unknown, fallback: SubtitleAnimationEasing): SubtitleAnimationEasing {
  return SUBTITLE_ANIMATION_EASINGS.includes(value as SubtitleAnimationEasing)
    ? (value as SubtitleAnimationEasing)
    : fallback;
}

// 旧版本只存 "microphone"/"system" 两个粗粒度值，迁移成新的 "kind:default" 格式，
// 避免下拉框因为找不到匹配项而显示成原始字符串。
function migrateLegacySource(source: string): string {
  if (source === "microphone") return buildSubtitleSource("mic");
  if (source === "system") return buildSubtitleSource("system");
  return source;
}

/**
 * 把任意输入夹到 [min, max]，只有**非有限数**才回退到 fallback。
 *
 * 原先统一写成 `Number(x) || fallback`，而 `0 || fallback` 取的是 fallback——
 * 于是下限为 0 或负数的档位永远设不上去：背景不透明度拖到 0 会弹回 72、
 * 圆角拖到 0 会弹回 18、位置偏移设到 0 会弹回 6，而且错误值还会被持久化。
 * 这三档恰好是 OBS / 录屏最常用的设置。null / undefined / 空串按缺省处理，
 * 因为 `Number(null) === 0` 会把「没存过」误读成用户选了 0。
 */
function clampNumber(value: unknown, min: number, max: number, fallback: number): number {
  const parsed = value === null || value === undefined || value === "" ? Number.NaN : Number(value);
  return Math.min(max, Math.max(min, Number.isFinite(parsed) ? parsed : fallback));
}

function clampPrefs(prefs: SubtitlePrefs): SubtitlePrefs {
  return {
    ...prefs,
    asrModel: isSupportedRealtimeModel(prefs.asrModel) ? prefs.asrModel : DEFAULT_REALTIME_ASR_MODEL,
    source: migrateLegacySource(prefs.source),
    fontSizePercent: clampNumber(prefs.fontSizePercent, 1.5, 6, 2.6),
    lineCount: Math.round(clampNumber(prefs.lineCount, 1, 4, 1)),
    widthPercent: clampNumber(prefs.widthPercent, 20, 70, 46),
    offsetYPercent: clampNumber(prefs.offsetYPercent, -17, 20, 6),
    backgroundOpacity: clampNumber(prefs.backgroundOpacity, 0, 100, 72),
    rounded: clampNumber(prefs.rounded, 0, 36, 18),
    motionEnabled: prefs.motionEnabled !== false,
    motionDurationMs: clampNumber(prefs.motionDurationMs, 60, 400, 120),
    motionEasing: clampEasing(prefs.motionEasing, "ease-out"),
    fadeEnabled: prefs.fadeEnabled !== false,
    fadeDurationMs: clampNumber(prefs.fadeDurationMs, 60, 500, 180),
    fadeEasing: clampEasing(prefs.fadeEasing, "ease-out"),
    translationModel: normalizeTranslationModel(prefs.translationModel),
    translationSourceLang: prefs.translationSourceLang || DEFAULT_TRANSLATION_SOURCE_LANG,
    translationTargetLang: prefs.translationTargetLang || DEFAULT_TRANSLATION_TARGET_LANG,
    translationLayout: prefs.translationLayout === "translationOnly" ? "translationOnly" : "bilingual",
    translationOrder: prefs.translationOrder === "translationFirst" ? "translationFirst" : "sourceFirst",
    obsOutputEnabled: prefs.obsOutputEnabled === true,
  };
}

function readStored(): SubtitlePrefs {
  const base = defaults();
  try {
    const raw = localStorage.getItem(SUBTITLE_PREFS_KEY);
    if (raw) Object.assign(base, JSON.parse(raw));
  } catch {
    /* noop */
  }
  return clampPrefs(base);
}

function persist(prefs: SubtitlePrefs) {
  try {
    localStorage.setItem(SUBTITLE_PREFS_KEY, JSON.stringify(prefs));
  } catch {
    /* noop */
  }
}

export const useSubtitleStore = create<SubtitleState>((set, get) => ({
  prefs: readStored(),
  running: false,
  previewActive: false,
  statusText: "实时字幕未开启",
  statusTone: "",
  latestText: "",
  capturing: false,
  shortcutLabel: "",
  obsOutputActive: false,
  patch: (partial) => {
    const next = clampPrefs({ ...get().prefs, ...partial });
    persist(next);
    set({ prefs: next });
    void cmd(CMD.updateAppSettings, { domain: "subtitles", value: next }).then(async () => {
      if (get().running) await cmd(CMD.syncSubtitlePresentation).catch(() => undefined);
    });
  },
  loadTranslationModel: async () => {
    const model = await cmd<string>(CMD.getSubtitleTranslationModel);
    const prefs = clampPrefs({ ...get().prefs, translationModel: model });
    persist(prefs);
    set({ prefs });
  },
  setTranslationModel: async (model) => {
    const normalized = normalizeTranslationModel(model);
    await cmd(CMD.setSubtitleTranslationModel, { model: normalized });
    const prefs = clampPrefs({ ...get().prefs, translationModel: normalized });
    await cmd(CMD.updateAppSettings, { domain: "subtitles", value: prefs });
    persist(prefs);
    set({ prefs });
    if (get().running) await cmd(CMD.syncSubtitlePresentation).catch(() => undefined);
  },
  setRuntime: (partial) => set(partial),
}));

export function hydrateSubtitlePrefs(value: Record<string, unknown>) {
  const next = clampPrefs({ ...readStored(), ...value } as SubtitlePrefs); persist(next); useSubtitleStore.setState({ prefs: next });
}
