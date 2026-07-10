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
// 一律丢弃，避免串场。segmentSeq 按分句单调递增，用于把乱序到达的译文事件放回正确位置。
let translationRequestId: string | null = null;
let translationEpochCounter = 0;
let translationSegmentSeq = 0;
let translations: Map<number, string> = new Map();
// 不整句等 ASR 判定 final 才翻译：currentSegment 里每出现一个分句切分点（标点/字数上限）就
// 立即派发翻译，partialTranslateOffset 记录 currentSegment 内已经切出去过的字符数，避免重复翻译。
let partialTranslateOffset = 0;
// 当前这句（尚未 final）已派发翻译的分句 seq，按顺序拼起来（不加分隔符）就是这句的完整译文；
// ASR 判定 final 时整体封存进下面两个结构之一，随后清空开始下一句。
let currentTranslationGroup: number[] = [];
// 与 committedLines 逐行一一对应（滚动模式用），每个元素是该行内各分句 seq 的列表。
let committedTranslationGroups: number[][] = [];
// 与 replaceModeLine 当前这组一一对应（单句替换模式用），组内多句原文按同样的续接/清空规则累积。
let replaceTranslationGroups: number[][] = [];
let translationDisplayText = "";

const MAX_RECONNECT_ATTEMPTS = 6;
let reconnecting = false;
let reconnectAttempts = 0;

let backendSystemAudioSampleRate = 48000;

const SUBTITLE_SHADOW_GUTTER = 0;
const SUBTITLE_PANEL_VERTICAL_PADDING = 28;
// 与 indicator.css 里 #translation-text 的 padding（10px 22px）、#wrap.subtitle-mode 的 gap（10px）对应，
// 双语模式下必须按这两个值预留窗口高度，否则译文行会被窗口边界裁掉。
const TRANSLATION_PANEL_VERTICAL_PADDING = 20;
const SUBTITLE_ROW_GAP = 10;
const LEGACY_SUBTITLE_TOP_PADDING = 18;
const LEGACY_SUBTITLE_BOTTOM_PADDING = 24;

function subtitleWindowOffset(_anchor: SubtitlePrefs["anchor"], offsetY: number) {
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
  await cmdSilent(CMD.ensureObsSubtitleCaptureWindow);
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
  // 双语模式下译文行和原文字号、行数（effectiveLines）完全一致，高度预留必须与 indicator.css 的
  // #translation-text 实际渲染尺寸一致，否则内容会被窗口边界裁掉；仅译文模式复用主通道，不需要额外空间。
  const translationEnabled = prefs.translationModel !== TRANSLATION_MODEL_NONE;
  const showsTranslationRow = translationEnabled && prefs.translationLayout === "bilingual";
  const extraHeight = showsTranslationRow
    ? lineHeight * effectiveLines + TRANSLATION_PANEL_VERTICAL_PADDING + SUBTITLE_ROW_GAP
    : 0;
  const height = lineHeight * effectiveLines + extraHeight + SUBTITLE_PANEL_VERTICAL_PADDING + SUBTITLE_SHADOW_GUTTER * 2;
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
      windowWidth,
      windowHeight: height,
      anchor: prefs.anchor,
      offsetY: subtitleWindowOffset(prefs.anchor, offsetY),
      motionEnabled: prefs.motionEnabled,
      motionDurationMs: prefs.motionDurationMs,
      motionEasing: prefs.motionEasing,
      fadeEnabled: prefs.fadeEnabled,
      fadeDurationMs: prefs.fadeDurationMs,
      fadeEasing: prefs.fadeEasing,
      translationEnabled,
      translationLayout: prefs.translationLayout,
      translationOrder: prefs.translationOrder,
    },
  });
}

// ---------------- 字幕预览：模拟播放 ----------------
// 预览不启动麦克风/识别、也不发起真实翻译请求（不产生网络调用和费用），而是用打字机式的
// 定时器模拟"识别中逐字增长→定稿→（如果开着翻译）译文流式吐出"的完整过程，复用与真实字幕
// 完全相同的显示状态（committedLines/replaceModeLine/翻译分组等）和渲染函数，这样字幕预览时
// 看到的动画效果（滚动、平移、淡入等）与真实使用时完全一致，多句循环播放也能看出滚动模式的
// 多行滚动效果。
const PREVIEW_SCRIPT: { source: string; translation: string }[] = [
  { source: "嗨，很高兴认识你，这是实时字幕的预览效果。", translation: "Hi, nice to meet you — this is a preview of the live captions." },
  { source: "你可以在这里调整字体、颜色、位置和动画，所见即所得。", translation: "You can adjust the font, color, position and animation here, and see the result instantly." },
  { source: "开启字幕翻译后，识别到的内容会实时翻译成你选择的语言。", translation: "Once translation is turned on, recognized speech is translated into your chosen language in real time." },
  { source: "调整满意后，点击开始字幕就可以正式使用啦。", translation: "Once you're happy with the look, just click Start Captions to begin using it." },
];
const PREVIEW_CHAR_INTERVAL_MS = 60;
const PREVIEW_SENTENCE_GAP_MS = 900;
// 每轮脚本播完后停顿更久（超过单句替换模式"续接"的阈值），顺带演示一下长时间停顿后清空重开一行的效果。
const PREVIEW_LOOP_GAP_MS = 3000;
const PREVIEW_TRANSLATE_DELAY_MS = 260;
const PREVIEW_TRANSLATE_CHAR_INTERVAL_MS = 26;

let previewTimer: ReturnType<typeof setTimeout> | null = null;
let previewActive = false;
let previewScriptIndex = 0;

function clearPreviewTimer() {
  if (previewTimer) {
    clearTimeout(previewTimer);
    previewTimer = null;
  }
}

/** 模拟译文"流式吐字"：从第 1 个字符逐步增长到完整译文，效果与真实分句翻译陆续到达时一致。 */
function playPreviewTranslation(text: string, group: number[]) {
  let pos = 0;
  const segSeq = ++translationSegmentSeq;
  group.push(segSeq);
  const tick = () => {
    if (!previewActive) return;
    pos += 1;
    translations.set(segSeq, text.slice(0, pos));
    renderTranslation();
    if (pos < text.length) previewTimer = setTimeout(tick, PREVIEW_TRANSLATE_CHAR_INTERVAL_MS);
  };
  previewTimer = setTimeout(tick, PREVIEW_TRANSLATE_DELAY_MS);
}

/** 模拟原文"识别中逐字增长"，定稿后按当前模式归档（与 handleSubtitleAsrEvent 的定稿分支逻辑保持一致），
 * 再视情况模拟这句的译文，然后进入下一句、循环播放。 */
function playPreviewSentence() {
  if (!previewActive) return;
  const item = PREVIEW_SCRIPT[previewScriptIndex % PREVIEW_SCRIPT.length];
  let pos = 0;
  const typeChar = () => {
    if (!previewActive) return;
    pos += 1;
    const partial = item.source.slice(0, pos);
    currentSegment = partial;
    renderSubtitle(partial);
    if (pos < item.source.length) {
      previewTimer = setTimeout(typeChar, PREVIEW_CHAR_INTERVAL_MS);
      return;
    }
    const prefs = useSubtitleStore.getState().prefs;
    const finished = item.source;
    committedLines.push(finished);
    committedLines = committedLines.slice(-12);
    committedTranslationGroups.push(currentTranslationGroup);
    committedTranslationGroups = committedTranslationGroups.slice(-12);
    if (prefs.mode === "replace") {
      const continuingGroup = !!replaceModeLine;
      replaceModeLine = replaceModeLine ? `${replaceModeLine}${REPLACE_LINE_SEPARATOR}${finished}` : finished;
      if (replaceModeLine.length > REPLACE_LINE_MAX_CHARS) {
        replaceModeLine = replaceModeLine.slice(-REPLACE_LINE_MAX_CHARS);
      }
      replaceModeLineAt = Date.now();
      replaceTranslationGroups = continuingGroup
        ? [...replaceTranslationGroups, currentTranslationGroup]
        : [currentTranslationGroup];
    }
    const sealedGroup = currentTranslationGroup;
    currentSegment = "";
    currentTranslationGroup = [];
    renderSubtitle("");

    if (prefs.translationModel !== TRANSLATION_MODEL_NONE) {
      playPreviewTranslation(item.translation, sealedGroup);
    }

    const isLastOfLoop = previewScriptIndex % PREVIEW_SCRIPT.length === PREVIEW_SCRIPT.length - 1;
    previewScriptIndex += 1;
    previewTimer = setTimeout(playPreviewSentence, isLastOfLoop ? PREVIEW_LOOP_GAP_MS : PREVIEW_SENTENCE_GAP_MS);
  };
  previewTimer = setTimeout(typeChar, PREVIEW_CHAR_INTERVAL_MS);
}

function startPreviewSimulation() {
  previewActive = true;
  previewScriptIndex = 0;
  committedLines = [];
  currentSegment = "";
  displayText = "";
  replaceModeLine = "";
  replaceModeLineAt = 0;
  partialTranslateOffset = 0;
  currentTranslationGroup = [];
  committedTranslationGroups = [];
  replaceTranslationGroups = [];
  translations = new Map();
  translationSegmentSeq = 0;
  translationDisplayText = "";
  playPreviewSentence();
}

function stopPreviewSimulation() {
  previewActive = false;
  clearPreviewTimer();
}

/** 在桌面悬浮窗里按当前样式模拟播放示例内容，不启动麦克风/识别、不产生真实翻译请求。真正开着字幕时不干预。 */
export async function showSubtitlePreview(prefs: SubtitlePrefs) {
  if (useSubtitleStore.getState().running) return;
  await syncSubtitleIndicator(prefs);
  cmdSilent(CMD.setIndicatorState, { state: "subtitle" });
  startPreviewSimulation();
}

/** 关闭预览悬浮窗，恢复到指示器的默认（隐藏）状态。 */
export async function hideSubtitlePreview() {
  // 无论是"用户关闭预览开关"还是"预览开着时用户直接开始了真字幕"，模拟播放都必须立即停掉，
  // 否则模拟定时器会继续往 committedLines/currentSegment 等共享状态里写东西，和真实识别打架。
  stopPreviewSimulation();
  if (useSubtitleStore.getState().running) return;
  await emitEvent(EVT.indicatorConfig, { mode: "dictation" });
  await cmdSilent(CMD.setIndicatorLayout, { width: 460, height: 188, anchor: "bottom", offsetY: 36 });
  await cmdSilent(CMD.setIndicatorState, { state: "hidden" });
  await cmdSilent(CMD.setIndicatorText, { text: "" });
  await cmdSilent(CMD.setIndicatorTranslation, { text: "" });
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

/** 把一个分句组内各 seq 的已知译文按顺序拼接（不加分隔符，组内本就是同一句话的连续片段）。 */
function joinTranslationGroup(group: number[]): string {
  return group
    .map((seq) => translations.get(seq))
    .filter((text): text is string => !!text)
    .join("");
}

/** 按分组顺序拼出译文显示串：自动按已知最新译文重建，与到达顺序无关，天然纠正乱序/增量。 */
function renderTranslation() {
  const prefs = useSubtitleStore.getState().prefs;
  let next: string;
  if (prefs.mode === "replace") {
    next = [...replaceTranslationGroups, currentTranslationGroup]
      .map(joinTranslationGroup)
      .filter(Boolean)
      .join(REPLACE_LINE_SEPARATOR);
    if (next.length > REPLACE_LINE_MAX_CHARS) next = next.slice(-REPLACE_LINE_MAX_CHARS);
  } else {
    next = [...committedTranslationGroups.map(joinTranslationGroup), joinTranslationGroup(currentTranslationGroup)]
      .filter(Boolean)
      .join("\n");
  }
  translationDisplayText = next.length > 1800 ? next.slice(-1800).replace(/^\s+/, "") : next;
  pushIndicatorChannels();
}

/** 对一段文本发起翻译；未开启翻译或字幕未在运行（无有效会话代次）时直接跳过。 */
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

const CLAUSE_HARD_END_RE = /[。！？；….!?]/;
/** 找不到标点时，累计到这个字符数也强制切一段，避免长句子/不带标点的识别结果一直等不到翻译。 */
const CLAUSE_MAX_CHARS = 60;

/**
 * 在 tail 里找一个"可以先送去翻译"的切分点：优先句末标点（。！？；…. ! ?）；
 * 其次退而求其次，累计到第二个逗号也切一次；再退一步，字数超过上限也强制切。
 * 用分句代替"整句等 ASR 判定 final 才翻译"，明显降低可感知的翻译延迟。
 */
function findClauseCut(tail: string): number {
  let lastHardEnd = -1;
  for (let i = 0; i < tail.length; i += 1) {
    if (CLAUSE_HARD_END_RE.test(tail[i])) lastHardEnd = i;
  }
  if (lastHardEnd >= 0) return lastHardEnd + 1;
  let commaCount = 0;
  for (let i = 0; i < tail.length; i += 1) {
    if (tail[i] === "，" || tail[i] === ",") {
      commaCount += 1;
      if (commaCount >= 2) return i + 1;
    }
  }
  if (tail.length >= CLAUSE_MAX_CHARS) return tail.length;
  return -1;
}

/**
 * 扫描 currentSegment 里新出现、还没送去翻译的部分，按标点切出可翻译的分句立即派发。
 * isFinal=true（ASR 已判定整句定稿）时把剩余尾巴也当作最后一个分句送出去，不再等标点。
 * ASR 的 partial 结果存在小概率回改早前文字的情况，这里按"标点处基本已经稳定"的假设
 * 换取更快的翻译速度，偶发的极小概率回改不做特殊处理。
 */
function dispatchClauseTranslations(isFinal: boolean) {
  const prefs = useSubtitleStore.getState().prefs;
  if (prefs.translationModel === TRANSLATION_MODEL_NONE || !translationRequestId) return;
  let tail = currentSegment.slice(partialTranslateOffset);
  for (;;) {
    const cut = findClauseCut(tail);
    if (cut <= 0) break;
    const clause = tail.slice(0, cut).trim();
    partialTranslateOffset += cut;
    tail = tail.slice(cut);
    if (!clause) continue;
    const segSeq = ++translationSegmentSeq;
    currentTranslationGroup.push(segSeq);
    requestSubtitleTranslation(clause, segSeq);
  }
  if (isFinal) {
    partialTranslateOffset = currentSegment.length;
    const rest = tail.trim();
    if (rest) {
      const segSeq = ++translationSegmentSeq;
      currentTranslationGroup.push(segSeq);
      requestSubtitleTranslation(rest, segSeq);
    }
  }
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
  // 保险起见：无论调用方是否已经通过 hideSubtitlePreview 关掉预览，真会话开始前一律先停掉
  // 模拟播放的定时器，避免它继续往下面这些共享状态里写东西、和真实识别的写入互相打架。
  stopPreviewSimulation();
  const prefs = useSubtitleStore.getState().prefs;
  committedLines = [];
  currentSegment = "";
  displayText = "";
  replaceModeLine = "";
  replaceModeLineAt = 0;
  reconnectAttempts = 0;
  partialTranslateOffset = 0;
  currentTranslationGroup = [];
  committedTranslationGroups = [];
  replaceTranslationGroups = [];
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
  partialTranslateOffset = 0;
  currentTranslationGroup = [];
  committedTranslationGroups = [];
  replaceTranslationGroups = [];
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
      // 不等整句 final：按标点把已经出现的分句立即送去翻译，明显降低可感知延迟。
      dispatchClauseTranslations(false);
    }
    if (data.payload?.final && currentSegment.trim()) {
      const finished = currentSegment.trim();
      // 收尾：把还没被标点切出去的尾巴也当作最后一个分句送去翻译。
      dispatchClauseTranslations(true);
      committedLines.push(finished);
      committedLines = committedLines.slice(-12);
      committedTranslationGroups.push(currentTranslationGroup);
      committedTranslationGroups = committedTranslationGroups.slice(-12);
      if (useSubtitleStore.getState().prefs.mode === "replace") {
        // 与原文 replaceModeLine 的分组决策保持一致：本轮开始前 replaceModeLine 已被清空
        // 说明这是新的一组（停顿超过阈值），否则是接着上一组继续。
        const continuingGroup = !!replaceModeLine;
        replaceModeLine = replaceModeLine ? `${replaceModeLine}${REPLACE_LINE_SEPARATOR}${finished}` : finished;
        if (replaceModeLine.length > REPLACE_LINE_MAX_CHARS) {
          replaceModeLine = replaceModeLine.slice(-REPLACE_LINE_MAX_CHARS);
        }
        replaceModeLineAt = Date.now();
        replaceTranslationGroups = continuingGroup
          ? [...replaceTranslationGroups, currentTranslationGroup]
          : [currentTranslationGroup];
      }
      currentSegment = "";
      partialTranslateOffset = 0;
      currentTranslationGroup = [];
      renderSubtitle("");
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
