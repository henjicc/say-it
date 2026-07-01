import { CMD, EVT, cmd, cmdSilent, emitEvent } from "@/lib/tauri";
import { useDictPrefs } from "@/store/useDictPrefs";
import { useProviderStore } from "@/store/useProviderStore";
import { useSubtitleStore, parseSubtitleSource, type SubtitlePrefs } from "@/store/useSubtitleStore";
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
} from "./hotkeys";

export {
  startSubtitleShortcutCapture,
  clearSubtitleShortcut,
  isSubtitleCapturing,
  loadSubtitleShortcut,
  installSubtitleFocusHotkeyFallback,
  handleForwardedSubtitleKeydown,
  handleForwardedSubtitleKeyup,
} from "./hotkeys";

let subtitleSessionId: string | null = null;
let subtitleSampleRate = 48000;
let busy = false;
let committedLines: string[] = [];
let currentSegment = "";
let displayText = "";

const MAX_RECONNECT_ATTEMPTS = 6;
let reconnecting = false;
let reconnectAttempts = 0;

let backendSystemAudioSampleRate = 48000;

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
  const height = Math.max(136, lineHeight * effectiveLines + 86);
  await cmdSilent(CMD.setIndicatorLayout, {
    width,
    height,
    anchor: prefs.anchor,
    offsetY,
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
  await cmdSilent(CMD.setIndicatorLayout, { width: 520, height: 220, anchor: "bottom", offsetY: 36 });
  await cmdSilent(CMD.setIndicatorState, { state: "hidden" });
  await cmdSilent(CMD.setIndicatorText, { text: "" });
}

function renderSubtitle(nextSegment = currentSegment) {
  const prefs = useSubtitleStore.getState().prefs;
  const stable = committedLines.join("\n");
  const next =
    prefs.mode === "replace"
      ? nextSegment || committedLines[committedLines.length - 1] || ""
      : [stable, nextSegment].filter(Boolean).join(stable && nextSegment ? "\n" : "");
  displayText = next.length > 1800 ? next.slice(-1800).replace(/^\s+/, "") : next;
  useSubtitleStore.getState().setRuntime({ latestText: displayText });
  cmdSilent(CMD.setIndicatorText, { text: displayText });
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
  reconnectAttempts = 0;
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
  await cmdSilent(CMD.pauseBackendMic);
  scheduleMicShutdown(pushLog);
  await cmdSilent(CMD.pauseBackendSystemAudio);
  await cmdSilent(CMD.releaseBackendSystemAudio);
  if (session) await cmdSilent(CMD.stopAsrStream, { sessionId: session });
  await emitEvent(EVT.indicatorConfig, { mode: "dictation" });
  await cmdSilent(CMD.setIndicatorLayout, { width: 520, height: 220, anchor: "bottom", offsetY: 36 });
  await cmdSilent(CMD.setIndicatorState, { state: "hidden" });
  await cmdSilent(CMD.setIndicatorText, { text: "" });
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
      currentSegment = text;
      renderSubtitle(text);
    }
    if (data.payload?.final && currentSegment.trim()) {
      committedLines.push(currentSegment.trim());
      committedLines = committedLines.slice(-12);
      currentSegment = "";
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
