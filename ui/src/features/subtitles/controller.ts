import { CMD, EVT, cmd, cmdSilent, emitEvent } from "@/lib/tauri";
import { useSubtitleStore, type SubtitlePrefs } from "@/store/useSubtitleStore";
import { TRANSLATION_MODEL_NONE } from "@/features/translation/models";
import {
  configureSubtitleHotkeys,
  startSubtitleShortcutCapture,
  clearSubtitleShortcut,
  isSubtitleCapturing,
  loadSubtitleShortcut,
  installSubtitleFocusHotkeyFallback,
  handleForwardedSubtitleKeydown,
  handleForwardedSubtitleKeyup,
  handleSubtitleCaptureLockKey,
} from "./hotkeys";

export {
  startSubtitleShortcutCapture,
  clearSubtitleShortcut,
  isSubtitleCapturing,
  loadSubtitleShortcut,
  installSubtitleFocusHotkeyFallback,
  handleForwardedSubtitleKeydown,
  handleForwardedSubtitleKeyup,
  handleSubtitleCaptureLockKey,
} from "./hotkeys";

export interface SubtitleRuntimeSnapshot {
  phase: "idle" | "waitingForVoice" | "running" | "reconnecting" | "stopping" | "failed";
  sessionId?: string;
  originalText: string;
  translationText: string;
  obsOutputActive: boolean;
  error?: string;
}

function applyRuntime(snapshot: SubtitleRuntimeSnapshot) {
  const running = !["idle", "failed"].includes(snapshot.phase);
  const waiting = snapshot.phase === "waitingForVoice";
  const reconnecting = snapshot.phase === "reconnecting";
  const failed = snapshot.phase === "failed";
  useSubtitleStore.getState().setRuntime({
    running,
    latestText: snapshot.originalText || "",
    obsOutputActive: snapshot.obsOutputActive === true,
    statusText: failed
      ? snapshot.error || "实时字幕运行失败"
      : reconnecting
        ? "实时字幕重新连接中…"
        : waiting
          ? "实时字幕已开启，正在等待声音…"
          : running
            ? "实时字幕已开启"
            : "实时字幕未开启",
    statusTone: failed ? "err" : running ? "ok" : "",
  });
}

export async function loadSubtitleRuntime() {
  applyRuntime(await cmd<SubtitleRuntimeSnapshot>(CMD.getSubtitleRuntime));
}

export function applySubtitleRuntime(snapshot: SubtitleRuntimeSnapshot) {
  applyRuntime(snapshot);
}

configureSubtitleHotkeys({
  setStatus: (statusText, statusTone = "") => useSubtitleStore.getState().setRuntime({ statusText, statusTone }),
  toggle: () => toggleSubtitles(),
});

export function handleSubtitleShortcutError(payload: { key_code?: string; message?: string }) {
  useSubtitleStore.getState().setRuntime({
    statusText: `实时字幕快捷键注册失败（${payload.key_code || "?"}）：${payload.message || "未知错误"}`,
    statusTone: "err",
  });
}

export async function toggleSubtitles() {
  stopPreviewSimulation();
  try {
    await cmd(CMD.subtitleToggle);
    await loadSubtitleRuntime().catch(() => undefined);
  } catch (error) {
    useSubtitleStore.getState().setRuntime({ statusText: `实时字幕切换失败：${String(error)}`, statusTone: "err" });
  }
}

export async function shutdownSubtitles() {
  await cmdSilent(CMD.subtitleStop);
  await loadSubtitleRuntime().catch(() => undefined);
}

export async function applyObsOutputRouting() {
  await cmdSilent(CMD.applySubtitleObsRouting);
}

export function rgba(hex: string, opacity: number) {
  const value = hex.replace("#", "").trim();
  const full = value.length === 3 ? value.split("").map((v) => `${v}${v}`).join("") : value.padEnd(6, "0").slice(0, 6);
  const r = parseInt(full.slice(0, 2), 16) || 0;
  const g = parseInt(full.slice(2, 4), 16) || 0;
  const b = parseInt(full.slice(4, 6), 16) || 0;
  return `rgba(${r}, ${g}, ${b}, ${Math.max(0, Math.min(1, opacity / 100))})`;
}

const PREVIEW_SCRIPT = [
  ["嗨，很高兴认识你，这是实时字幕的预览效果。", "Hi, nice to meet you — this is a preview of the live captions."],
  ["你可以在这里调整字体、颜色、位置和动画，所见即所得。", "You can adjust the font, color, position and animation here, and see the result instantly."],
  ["开启字幕翻译后，识别到的内容会实时翻译成你选择的语言。", "Once translation is turned on, recognized speech is translated into your chosen language in real time."],
  ["调整满意后，点击开始字幕就可以正式使用啦。", "Once you're happy with the look, just click Start Captions to begin using it."],
] as const;

const previewTimers = new Set<ReturnType<typeof setTimeout>>();
let previewActive = false;
let previewGeneration = 0;
let previewIndex = 0;
let previewCommitted: string[] = [];
let previewOriginal = "";
let previewTranslation = "";

function clearPreviewTimers() {
  for (const timer of previewTimers) clearTimeout(timer);
  previewTimers.clear();
}

function schedulePreview(callback: () => void, delay: number, generation: number) {
  const timer = setTimeout(() => {
    previewTimers.delete(timer);
    if (previewActive && generation === previewGeneration) callback();
  }, delay);
  previewTimers.add(timer);
}

function previewDisplay(prefs: SubtitlePrefs, current = "") {
  if (prefs.mode === "replace") return current || previewCommitted.at(-1) || "";
  return [...previewCommitted, current].filter(Boolean).slice(-prefs.lineCount).join("\n");
}

function pushPreviewChannels(prefs: SubtitlePrefs) {
  if (prefs.translationModel === TRANSLATION_MODEL_NONE) {
    cmdSilent(CMD.setIndicatorText, { text: previewOriginal });
    cmdSilent(CMD.setIndicatorTranslation, { text: "" });
  } else if (prefs.translationLayout === "translationOnly") {
    cmdSilent(CMD.setIndicatorText, { text: previewTranslation });
    cmdSilent(CMD.setIndicatorTranslation, { text: "" });
  } else {
    cmdSilent(CMD.setIndicatorText, { text: previewOriginal });
    cmdSilent(CMD.setIndicatorTranslation, { text: previewTranslation });
  }
}

function playPreviewSentence(generation: number) {
  const [source, translation] = PREVIEW_SCRIPT[previewIndex % PREVIEW_SCRIPT.length];
  const prefs = useSubtitleStore.getState().prefs;
  let sourcePos = 0;
  const typeSource = () => {
    sourcePos += 1;
    previewOriginal = previewDisplay(prefs, source.slice(0, sourcePos));
    pushPreviewChannels(prefs);
    if (sourcePos < source.length) return schedulePreview(typeSource, 60, generation);
    previewCommitted.push(source);
    previewCommitted = previewCommitted.slice(-12);
    previewOriginal = previewDisplay(prefs);
    if (prefs.translationModel !== TRANSLATION_MODEL_NONE) {
      let translationPos = 0;
      const typeTranslation = () => {
        translationPos += 1;
        previewTranslation = translation.slice(0, translationPos);
        pushPreviewChannels(prefs);
        if (translationPos < translation.length) schedulePreview(typeTranslation, 26, generation);
      };
      schedulePreview(typeTranslation, 260, generation);
    }
    const last = previewIndex % PREVIEW_SCRIPT.length === PREVIEW_SCRIPT.length - 1;
    previewIndex += 1;
    schedulePreview(() => playPreviewSentence(generation), last ? 3_000 : 900, generation);
  };
  schedulePreview(typeSource, 60, generation);
}

function stopPreviewSimulation() {
  previewGeneration += 1;
  previewActive = false;
  clearPreviewTimers();
}

async function syncPreviewIndicator(prefs: SubtitlePrefs) {
  const { width: monitorWidth, height: monitorHeight } = await cmd<{ width: number; height: number }>(CMD.getIndicatorMonitorMetrics)
    .catch(() => ({ width: 1920, height: 1080 }));
  const fontSize = Math.round((monitorHeight * prefs.fontSizePercent) / 100);
  const width = Math.round((monitorWidth * prefs.widthPercent) / 100);
  const offsetY = Math.round((monitorHeight * prefs.offsetYPercent) / 100);
  const lines = prefs.mode === "replace" ? 1 : prefs.lineCount;
  const lineHeight = Math.round(fontSize * 1.38);
  const translationEnabled = prefs.translationModel !== TRANSLATION_MODEL_NONE;
  const height = lineHeight * lines + 28 + (translationEnabled && prefs.translationLayout === "bilingual" ? lineHeight * lines + 30 : 0);
  await cmdSilent(CMD.setIndicatorLayout, { width, height, anchor: prefs.anchor, offsetY });
  await emitEvent(EVT.indicatorConfig, {
    mode: "subtitle",
    subtitle: {
      displayMode: prefs.mode, fontFamily: prefs.fontFamily, fontSize, lineCount: lines,
      textColor: prefs.textColor, backgroundColor: rgba(prefs.backgroundColor, prefs.backgroundOpacity),
      rounded: prefs.rounded, width, windowWidth: width, windowHeight: height, anchor: prefs.anchor, offsetY,
      motionEnabled: prefs.motionEnabled, motionDurationMs: prefs.motionDurationMs, motionEasing: prefs.motionEasing,
      fadeEnabled: prefs.fadeEnabled, fadeDurationMs: prefs.fadeDurationMs, fadeEasing: prefs.fadeEasing,
      translationEnabled, translationLayout: prefs.translationLayout, translationOrder: prefs.translationOrder,
    },
  });
  await cmdSilent(CMD.setIndicatorState, { state: "subtitle" });
  pushPreviewChannels(prefs);
}

export async function syncSubtitleIndicator(prefs: SubtitlePrefs = useSubtitleStore.getState().prefs) {
  if (useSubtitleStore.getState().running) {
    await cmdSilent(CMD.syncSubtitlePresentation);
  } else if (previewActive) {
    await syncPreviewIndicator(prefs);
  }
}

export async function showSubtitlePreview(prefs: SubtitlePrefs) {
  if (useSubtitleStore.getState().running) return;
  stopPreviewSimulation();
  const generation = ++previewGeneration;
  previewActive = true;
  previewIndex = 0;
  previewCommitted = [];
  previewOriginal = "";
  previewTranslation = "";
  await syncPreviewIndicator(prefs);
  if (generation === previewGeneration && previewActive) playPreviewSentence(generation);
}

export async function hideSubtitlePreview() {
  stopPreviewSimulation();
  if (useSubtitleStore.getState().running) return;
  previewCommitted = [];
  previewOriginal = "";
  previewTranslation = "";
  await emitEvent(EVT.indicatorConfig, { mode: "dictation" });
  await cmdSilent(CMD.setIndicatorLayout, { width: 460, height: 188, anchor: "bottom", offsetY: 36 });
  await cmdSilent(CMD.setIndicatorState, { state: "hidden" });
  await cmdSilent(CMD.setIndicatorText, { text: "" });
  await cmdSilent(CMD.setIndicatorTranslation, { text: "" });
}
