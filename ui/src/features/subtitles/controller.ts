import { CMD, EVT, cmd, cmdSilent, emitEvent } from "@/lib/tauri";
import { useDictPrefs } from "@/store/useDictPrefs";
import { useProviderStore } from "@/store/useProviderStore";
import { useSubtitleStore, parseSubtitleSource, type SubtitlePrefs } from "@/store/useSubtitleStore";
import { TRANSLATION_MODEL_NONE } from "@/features/translation/models";
import {
  clearMicShutdownTimer,
  ensureMic,
  getBackendMicSampleRate,
  scheduleMicShutdown,
  shutdownMic,
} from "@/features/dictation/micSession";
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

let subtitleSessionId: string | null = null;
let subtitleSampleRate = 48000;
let busy = false;
let committedLines: string[] = [];
let currentSegment = "";
let displayText = "";

// 单句替换模式下，当前这一行里"已经说完、确定不会再变"的文本——可能是跨了好几次
// 断句（说话中间停顿）拼起来的，只有停顿时间超过 REPLACE_LINE_CONTINUE_GAP_MS 才清空重开一行。
let replaceModeLine = "";
let replaceModeLineAt = 0;
const REPLACE_LINE_CONTINUE_GAP_MS = 2500;
const REPLACE_LINE_MAX_CHARS = 1800;
const REPLACE_LINE_SEPARATOR = " ";

// 字幕翻译：每次开始字幕分配一个新的会话代次（requestId），停止/重开后旧代次的迟到译文事件
// 一律丢弃，避免串场。segmentSeq 按定稿句单调递增，用于把乱序到达的译文事件放回正确位置。
let translationRequestId: string | null = null;
let translationEpochCounter = 0;
let translationSegmentSeq = 0;
let translations: Map<number, string> = new Map();
// 与 committedLines / replaceModeLine 分组一一对应的句子序号，供 renderTranslation() 按序拼出译文。
let committedSegmentSeqs: number[] = [];
let replaceLineSegmentSeqs: number[] = [];
let translationDisplayText = "";

const MAX_RECONNECT_ATTEMPTS = 6;
let reconnecting = false;
let reconnectAttempts = 0;

let backendSystemAudioSampleRate = 48000;

const SUBTITLE_SHADOW_GUTTER = 56;
const SUBTITLE_PANEL_VERTICAL_PADDING = 28;
const LEGACY_SUBTITLE_TOP_PADDING = 18;
const LEGACY_SUBTITLE_BOTTOM_PADDING = 24;

function subtitleWindowOffset(anchor: SubtitlePrefs["anchor"], offsetY: number) {
  if (anchor === "top") return offsetY - (SUBTITLE_SHADOW_GUTTER - LEGACY_SUBTITLE_TOP_PADDING);
  if (anchor === "bottom") return offsetY - (SUBTITLE_SHADOW_GUTTER - LEGACY_SUBTITLE_BOTTOM_PADDING);
  return offsetY;
}

function setStatus(statusText: string, statusTone: "" | "ok" | "err" = "") {
  useSubtitleStore.getState().setRuntime({ statusText, statusTone });
}

configureSubtitleHotkeys({
  setStatus,
  toggle: () => toggleSubtitles(),
});

export function handleSubtitleShortcutError(payload: { key_code?: string; message?: string }) {
  setStatus(`实时字幕快捷键注册失败（${payload.key_code || "?"}）：${payload.message || "未知错误"}`, "err");
}

function pushLog(message: string) {
  if (useDictPrefs.getState().prefs.debugLog) {
    console.log(`[subtitles] ${message}`);
  }
}

export function rgba(hex: string, opacity: number) {
  const value = hex.replace("#", "").trim();
  const full =
    value.length === 3
      ? value
          .split("")
          .map((v) => `${v}${v}`)
          .join("")
      : value.padEnd(6, "0").slice(0, 6);
  const r = parseInt(full.slice(0, 2), 16) || 0;
  const g = parseInt(full.slice(2, 4), 16) || 0;
  const b = parseInt(full.slice(4, 6), 16) || 0;
  return `rgba(${r}, ${g}, ${b}, ${Math.max(0, Math.min(1, opacity / 100))})`;
}

export async function syncSubtitleIndicator(prefs: SubtitlePrefs = useSubtitleStore.getState().prefs) {
  const { width: monitorWidth, height: monitorHeight } = await cmd<{ width: number; height: number }>(
    CMD.getIndicatorMonitorMetrics,
  ).catch(() => ({ width: 1920, height: 1080 }));
  const fontSize = Math.round((monitorHeight * prefs.fontSizePercent) / 100);
  const width = Math.round((monitorWidth * prefs.widthPercent) / 100);
  const offsetY = Math.round((monitorHeight * prefs.offsetYPercent) / 100);
  // 单句替换模式下永远只显示当前一行，行高不应受"显示行数"设置影响。
  const effectiveLines = prefs.mode === "replace" ? 1 : prefs.lineCount;
  const lineHeight = Math.round(fontSize * 1.38);
  const windowWidth = width + SUBTITLE_SHADOW_GUTTER * 2;
  // 双语模式需要给译文行额外留一行高度；仅译文模式复用主通道，不需要额外空间。
  const translationEnabled = prefs.translationModel !== TRANSLATION_MODEL_NONE;
  const showsTranslationRow = translationEnabled && prefs.translationLayout === "bilingual";
  const translationFontSize = Math.round(fontSize * 0.82);
  const translationLineHeight = Math.round(translationFontSize * 1.38);
  const extraHeight = showsTranslationRow ? translationLineHeight + 6 : 0;
  const height = Math.max(
    136,
    lineHeight * effectiveLines + extraHeight + SUBTITLE_PANEL_VERTICAL_PADDING + SUBTITLE_SHADOW_GUTTER * 2,
  );
  await cmdSilent(CMD.setIndicatorLayout, {
    width: windowWidth,
    height,
    anchor: prefs.anchor,
    offsetY: subtitleWindowOffset(prefs.anchor, offsetY),
  });
  await emitEvent(EVT.indicatorConfig, {
    mode: "subtitle",
    subtitle: {
      displayMode: prefs.mode,
      fontFamily: prefs.fontFamily,
      fontSize,
      lineCount: effectiveLines,
      textColor: prefs.textColor,
      backgroundColor: rgba(prefs.backgroundColor, prefs.backgroundOpacity),
      rounded: prefs.rounded,
      width,
      motionEnabled: prefs.motionEnabled,
      motionDurationMs: prefs.motionDurationMs,
      motionEasing: prefs.motionEasing,
      fadeEnabled: prefs.fadeEnabled,
      fadeDurationMs: prefs.fadeDurationMs,
      fadeEasing: prefs.fadeEasing,
      translationEnabled,
      translationLayout: prefs.translationLayout,
      translationOrder: prefs.translationOrder,
      translationFontSize,
    },
  });
}

export const SUBTITLE_PREVIEW_TEXT = "这是实时字幕预览效果，可在此调整样式，同步显示在桌面实际位置";

/** 在桌面悬浮窗里按当前样式展示示例文本，不启动麦克风/识别。真正开着字幕时不干预。 */
export async function showSubtitlePreview(prefs: SubtitlePrefs) {
  if (useSubtitleStore.getState().running) return;
  await syncSubtitleIndicator(prefs);
  cmdSilent(CMD.setIndicatorState, { state: "subtitle" });
  cmdSilent(CMD.setIndicatorText, { text: SUBTITLE_PREVIEW_TEXT });
}

/** 关闭预览悬浮窗，恢复到指示器的默认（隐藏）状态。真正开着字幕时不干预。 */
export async function hideSubtitlePreview() {
  if (useSubtitleStore.getState().running) return;
  await emitEvent(EVT.indicatorConfig, { mode: "dictation" });
  await cmdSilent(CMD.setIndicatorLayout, { width: 460, height: 188, anchor: "bottom", offsetY: 36 });
  await cmdSilent(CMD.setIndicatorState, { state: "hidden" });
  await cmdSilent(CMD.setIndicatorText, { text: "" });
}

/**
 * 把当前原文/译文按显示配置分别推给悬浮窗的两个文本通道：
 * - 未开启翻译：主通道=原文，副通道清空。
 * - 双语：主通道=原文，副通道=译文。
 * - 仅译文：主通道直接改发译文（复用原文那套富渲染动画），副通道清空。
 */
function pushIndicatorChannels() {
  const prefs = useSubtitleStore.getState().prefs;
  if (prefs.translationModel === TRANSLATION_MODEL_NONE) {
    cmdSilent(CMD.setIndicatorText, { text: displayText });
    cmdSilent(CMD.setIndicatorTranslation, { text: "" });
    return;
  }
  if (prefs.translationLayout === "translationOnly") {
    cmdSilent(CMD.setIndicatorText, { text: translationDisplayText });
    cmdSilent(CMD.setIndicatorTranslation, { text: "" });
    return;
  }
  cmdSilent(CMD.setIndicatorText, { text: displayText });
  cmdSilent(CMD.setIndicatorTranslation, { text: translationDisplayText });
}

function renderSubtitle(nextSegment = currentSegment) {
  const prefs = useSubtitleStore.getState().prefs;
  const stable = committedLines.join("\n");
  const next =
    prefs.mode === "replace"
      ? nextSegment
        ? replaceModeLine
          ? `${replaceModeLine}${REPLACE_LINE_SEPARATOR}${nextSegment}`
          : nextSegment
        : replaceModeLine
      : [stable, nextSegment].filter(Boolean).join(stable && nextSegment ? "\n" : "");
  displayText = next.length > 1800 ? next.slice(-1800).replace(/^\s+/, "") : next;
  useSubtitleStore.getState().setRuntime({ latestText: displayText });
  pushIndicatorChannels();
}

/** 按 segmentSeq 顺序拼出译文显示串：自动按已知最新译文重建，与到达顺序无关，天然纠正乱序/增量。 */
function renderTranslation() {
  const prefs = useSubtitleStore.getState().prefs;
  const seqs = prefs.mode === "replace" ? replaceLineSegmentSeqs : committedSegmentSeqs;
  const parts = seqs
    .map((seq) => translations.get(seq))
    .filter((text): text is string => !!text);
  const next = prefs.mode === "replace" ? parts.join(REPLACE_LINE_SEPARATOR) : parts.join("\n");
  translationDisplayText = next.length > 1800 ? next.slice(-1800).replace(/^\s+/, "") : next;
  pushIndicatorChannels();
}

/** 对一句已定稿的原文发起翻译；未开启翻译或字幕未在运行（无有效会话代次）时直接跳过。 */
function requestSubtitleTranslation(text: string, segmentSeq: number) {
  const prefs = useSubtitleStore.getState().prefs;
  if (prefs.translationModel === TRANSLATION_MODEL_NONE || !translationRequestId) return;
  cmdSilent(CMD.translateSubtitleStart, {
    request: {
      requestId: translationRequestId,
      segmentSeq,
      text,
      model: prefs.translationModel,
      sourceLang: prefs.translationSourceLang,
      targetLang: prefs.translationTargetLang,
    },
  });
}

/** 接收后端流式回传的译文事件；requestId 不匹配当前会话代次（已停止/重开）的一律丢弃。 */
export function handleSubtitleTranslationEvent(data: {
  requestId?: string;
  segmentSeq?: number;
  text?: string;
  done?: boolean;
  error?: string;
}) {
  if (!data.requestId || data.requestId !== translationRequestId) return;
  if (typeof data.segmentSeq !== "number") return;
  if (data.error) {
    pushLog(`字幕翻译失败：${data.error}`);
    return;
  }
  translations.set(data.segmentSeq, data.text || "");
  renderTranslation();
}

/** 原生 loopback 采集系统音频（把选定的播放设备当输入设备打开），不依赖浏览器共享屏幕弹窗。 */
async function ensureBackendSystemAudio(deviceName: string | undefined) {
  const result = await cmd<{
    sampleRate?: number;
    channels?: number;
    reused?: boolean;
    deviceName?: string | null;
    fallback?: boolean;
  }>(CMD.startBackendSystemAudio, { deviceName });
  backendSystemAudioSampleRate = result.sampleRate || 48000;
  pushLog(
    `系统音频采集已${result.reused ? "复用" : "激活"}：${backendSystemAudioSampleRate}Hz / ${result.channels || 1}ch${
      result.deviceName ? ` / ${result.deviceName}` : " / 默认播放设备"
    }`,
  );
  if (result.fallback) pushLog("所选播放设备未找到，已回退到默认播放设备。");
}

/** 开一路新的 ASR 会话，并把已在跑的对应后端音频采集（麦克风/系统音频）接过来。不动 committedLines/currentSegment，供首次启动和断线重连共用。 */
async function openAsrSession(prefs: SubtitlePrefs, sampleRate: number) {
  const session = await cmd<{ session_id: string }>(CMD.startAsrStream, {
    providerId: useProviderStore.getState().effective("asr"),
    modelOverride: prefs.asrModel,
    sampleRate,
    params: useDictPrefs.getState().dspParams(),
  });
  subtitleSessionId = session.session_id;
  const { kind } = parseSubtitleSource(prefs.source);
  if (kind === "mic") {
    await cmd(CMD.attachBackendMicToAsr, { sessionId: subtitleSessionId });
  } else {
    await cmd(CMD.attachBackendSystemAudioToAsr, { sessionId: subtitleSessionId });
  }
}

/**
 * 上游 ASR 连接结束后自动重开一路新会话，音频采集（麦克风/系统音频）不受影响，
 * 已识别的 committedLines 也不清空——这样"单句替换"模式下说完一句话不会卡死，
 * 后面继续说话会接上新的一句。连续失败达到上限才放弃并停止字幕。
 */
async function reconnectSubtitleSession() {
  if (reconnecting || !useSubtitleStore.getState().running) return;
  reconnecting = true;
  try {
    reconnectAttempts += 1;
    if (reconnectAttempts > MAX_RECONNECT_ATTEMPTS) {
      pushLog("自动重连次数过多，已停止实时字幕。");
      await stopSubtitles();
      setStatus("实时字幕连接反复中断，已自动停止，请检查网络或 API Key 后重新开始。", "err");
      return;
    }
    if (reconnectAttempts > 1) {
      await new Promise((resolve) => setTimeout(resolve, Math.min(2000, 300 * reconnectAttempts)));
    }
    pushLog(`ASR 连接已结束，正在自动重连（第 ${reconnectAttempts} 次）…`);
    setStatus("实时字幕重新连接中…");
    const prefs = useSubtitleStore.getState().prefs;
    await openAsrSession(prefs, subtitleSampleRate);
    if (!useSubtitleStore.getState().running) {
      // 重连期间用户已手动停止，收掉刚建好的会话，不要留成孤儿连接。
      const orphan = subtitleSessionId;
      subtitleSessionId = null;
      if (orphan) await cmdSilent(CMD.stopAsrStream, { sessionId: orphan });
      return;
    }
    pushLog("ASR 会话已自动重连。");
    setStatus(
      parseSubtitleSource(prefs.source).kind === "mic" ? "实时字幕已开启：麦克风" : "实时字幕已开启：系统音频",
      "ok",
    );
  } catch (error) {
    pushLog(`自动重连失败：${String(error)}`);
    await stopSubtitles();
    setStatus(`实时字幕连接已断开且自动重连失败：${String(error)}`, "err");
  } finally {
    reconnecting = false;
  }
}

async function startSubtitles() {
  const prefs = useSubtitleStore.getState().prefs;
  committedLines = [];
  currentSegment = "";
  displayText = "";
  replaceModeLine = "";
  replaceModeLineAt = 0;
  reconnectAttempts = 0;
  committedSegmentSeqs = [];
  replaceLineSegmentSeqs = [];
  translations = new Map();
  translationSegmentSeq = 0;
  translationDisplayText = "";
  translationRequestId = String(++translationEpochCounter);
  clearMicShutdownTimer();
  await syncSubtitleIndicator(prefs);

  subtitleSampleRate = 48000;
  const { kind, deviceName } = parseSubtitleSource(prefs.source);
  if (kind === "mic") {
    await ensureMic(pushLog);
    subtitleSampleRate = getBackendMicSampleRate() || 48000;
  } else {
    await ensureBackendSystemAudio(deviceName);
    subtitleSampleRate = backendSystemAudioSampleRate;
  }

  await openAsrSession(prefs, subtitleSampleRate);

  useSubtitleStore.getState().setRuntime({
    running: true,
    statusText: kind === "mic" ? "实时字幕已开启：麦克风" : "实时字幕已开启：系统音频",
    statusTone: "ok",
    latestText: "",
  });
  cmdSilent(CMD.setIndicatorState, { state: "subtitle" });
  cmdSilent(CMD.setIndicatorText, { text: "" });
}

async function stopSubtitles() {
  const session = subtitleSessionId;
  subtitleSessionId = null;
  currentSegment = "";
  committedLines = [];
  replaceModeLine = "";
  replaceModeLineAt = 0;
  translationRequestId = null;
  committedSegmentSeqs = [];
  replaceLineSegmentSeqs = [];
  translations = new Map();
  translationDisplayText = "";
  await cmdSilent(CMD.pauseBackendMic);
  scheduleMicShutdown(pushLog);
  await cmdSilent(CMD.pauseBackendSystemAudio);
  await cmdSilent(CMD.releaseBackendSystemAudio);
  if (session) await cmdSilent(CMD.stopAsrStream, { sessionId: session });
  await emitEvent(EVT.indicatorConfig, { mode: "dictation" });
  await cmdSilent(CMD.setIndicatorLayout, { width: 460, height: 188, anchor: "bottom", offsetY: 36 });
  await cmdSilent(CMD.setIndicatorState, { state: "hidden" });
  await cmdSilent(CMD.setIndicatorText, { text: "" });
  await cmdSilent(CMD.setIndicatorTranslation, { text: "" });
  useSubtitleStore.getState().setRuntime({
    running: false,
    statusText: "实时字幕已停止",
    statusTone: "",
  });
}

export async function toggleSubtitles() {
  if (busy) return;
  busy = true;
  try {
    if (useSubtitleStore.getState().running) await stopSubtitles();
    else await startSubtitles();
  } catch (error) {
    const session = subtitleSessionId;
    subtitleSessionId = null;
    translationRequestId = null;
    await shutdownMic();
    await cmdSilent(CMD.releaseBackendSystemAudio);
    if (session) await cmdSilent(CMD.stopAsrStream, { sessionId: session });
    await cmdSilent(CMD.setIndicatorState, { state: "hidden" });
    useSubtitleStore.getState().setRuntime({
      running: false,
      statusText: `实时字幕出错：${String(error)}`,
      statusTone: "err",
    });
  } finally {
    setTimeout(() => {
      busy = false;
    }, 250);
  }
}

export function handleSubtitleAsrEvent(data: {
  session_id?: string;
  kind?: string;
  payload?: { text?: string; final?: boolean; message?: string };
}): boolean {
  if (!data.session_id || data.session_id !== subtitleSessionId) return false;
  if (data.kind === "result") {
    reconnectAttempts = 0;
    const text = data.payload?.text || "";
    if (text) {
      if (!currentSegment && useSubtitleStore.getState().prefs.mode === "replace") {
        // 新一句话的第一个 partial：停顿超过阈值才当作新话题清空重开一行，
        // 否则接着贴在上一行后面，避免说话中间一停顿整行就被顶掉。
        if (Date.now() - replaceModeLineAt > REPLACE_LINE_CONTINUE_GAP_MS) replaceModeLine = "";
      }
      currentSegment = text;
      renderSubtitle(text);
    }
    if (data.payload?.final && currentSegment.trim()) {
      const finished = currentSegment.trim();
      const segSeq = ++translationSegmentSeq;
      committedLines.push(finished);
      committedLines = committedLines.slice(-12);
      committedSegmentSeqs.push(segSeq);
      committedSegmentSeqs = committedSegmentSeqs.slice(-12);
      if (useSubtitleStore.getState().prefs.mode === "replace") {
        // 与原文 replaceModeLine 的分组决策保持一致：本轮开始前 replaceModeLine 已被清空
        // 说明这是新的一组（停顿超过阈值），否则是接着上一组继续。
        const continuingGroup = !!replaceModeLine;
        replaceModeLine = replaceModeLine ? `${replaceModeLine}${REPLACE_LINE_SEPARATOR}${finished}` : finished;
        if (replaceModeLine.length > REPLACE_LINE_MAX_CHARS) {
          replaceModeLine = replaceModeLine.slice(-REPLACE_LINE_MAX_CHARS);
        }
        replaceModeLineAt = Date.now();
        replaceLineSegmentSeqs = continuingGroup ? [...replaceLineSegmentSeqs, segSeq] : [segSeq];
      }
      currentSegment = "";
      renderSubtitle("");
      requestSubtitleTranslation(finished, segSeq);
    }
  } else if (data.kind === "error") {
    // 上游 ASR 出错后 Rust 侧总会紧接着断开并触发下面的 "ended"，由那里统一负责自动重连，
    // 这里只记日志，避免每次瞬时错误都在界面上闪一次刺眼的红色提示。
    pushLog(`实时字幕 ASR 错误：${data.payload?.message || "未知错误"}`);
  } else if (data.kind === "ended") {
    if (useSubtitleStore.getState().running) reconnectSubtitleSession();
  }
  return true;
}

export async function shutdownSubtitles() {
  if (!useSubtitleStore.getState().running && !subtitleSessionId) return;
  await stopSubtitles();
}
