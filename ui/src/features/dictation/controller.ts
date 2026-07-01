import { CMD, cmd, cmdSilent } from "@/lib/tauri";
import { compactLogJson } from "@/lib/format";
import { useDictPrefs } from "@/store/useDictPrefs";
import { useDictationStore } from "@/store/useDictationStore";
import { useProviderStore } from "@/store/useProviderStore";
import { playCue } from "@/lib/cues";
import { runLocalRules } from "./localRules";
import {
  configureHotkeys,
  comboLabel,
  getInjectMethod,
  isCapturing,
  startShortcutCapture,
  setInjectMethod,
  handleForwardedKeydown,
  handleForwardedKeyup,
  installFocusHotkeyFallback,
  loadDictationSettings,
} from "./hotkeys";
import { pushIndicatorText, resetIndicatorPreview } from "./indicatorBridge";
import { clearMicShutdownTimer, ensureMic, getBackendMicSampleRate, scheduleMicShutdown, shutdownMic } from "./micSession";

export {
  startShortcutCapture,
  isCapturing,
  setInjectMethod,
  handleForwardedKeydown,
  handleForwardedKeyup,
  installFocusHotkeyFallback,
  loadDictationSettings,
} from "./hotkeys";

type Tone = "" | "ok" | "err";

let dictSessionId: string | null = null;
let dictRecording = false;
let dictBusy = false;
let dictAwaitingFinal = false;
let dictCommitted = "";
let dictSegment = "";
let dictFinalized = false;
let dictResultCount = 0;
let dictStartedAt = 0;
let dictFinalizeTimer: ReturnType<typeof setTimeout> | null = null;

function dspParams() {
  return useDictPrefs.getState().dspParams();
}

function setDictationStatus(text: string, tone: Tone = "") {
  useDictationStore.setState({ statusText: text, statusTone: tone });
}

function pushDictLog(message: string) {
  if (!useDictPrefs.getState().prefs.debugLog) return;
  const line = `${new Date().toLocaleTimeString()} ${message}`;
  console.log(`[dictation] ${message}`);
  const prev = useDictationStore.getState().log;
  const next = `${prev ? `${prev}\n` : ""}${line}`.split("\n").slice(-40).join("\n");
  useDictationStore.setState({ log: next });
}

export function clearDictLog() {
  useDictationStore.setState({ log: "" });
}

configureHotkeys({
  setStatus: setDictationStatus,
  getRecording: () => useDictationStore.getState().recording,
  isAssistantActive: () => false,
  toggleDictation: () => toggleDictation(),
  onCancelKey: () => onCancelKey(),
});

function scheduleDictFinalize(delay: number) {
  if (dictFinalizeTimer) clearTimeout(dictFinalizeTimer);
  dictFinalizeTimer = setTimeout(() => finalizeDictation(), delay);
}

function handleDictSegmentEnd(session: string) {
  if (session !== dictSessionId) return;
  finalizeDictation();
}

async function runLocalProcessing(text: string): Promise<string> {
  const prefs = useDictPrefs.getState().prefs;
  if (!prefs.localRulesEnabled) return text;
  const active = prefs.localRules.filter((r) => r.enabled && r.pattern).length;
  if (active === 0) return text;
  setDictationStatus("识别完成，正在本地处理…");
  const result = await runLocalRules(text, prefs.localRules);
  const out = result.text.trim() || text;
  if (result.timedOut) {
    pushDictLog("本地处理超时，已回退原文。");
  } else if (result.error) {
    pushDictLog(`本地处理出错，已回退原文：${result.error}`);
  } else {
    pushDictLog(`本地处理：${text.length} → ${out.length} 字（启用规则 ${active} 条）`);
  }
  return out;
}

async function startDictation() {
  if (dictRecording) return;
  clearMicShutdownTimer();
  dictCommitted = "";
  dictSegment = "";
  dictFinalized = false;
  dictAwaitingFinal = false;
  dictResultCount = 0;
  dictStartedAt = Date.now();
  resetIndicatorPreview();

  const t0 = Date.now();
  await ensureMic(pushDictLog);
  const micMs = Date.now() - t0;

  const session = await cmd<{ session_id: string }>(CMD.startAsrStream, {
    providerId: useProviderStore.getState().effective("asr"),
    sampleRate: getBackendMicSampleRate() || 48000,
    params: dspParams(),
  });
  dictSessionId = session.session_id;
  const attach = await cmd<{ flushedChunks?: number }>(CMD.attachBackendMicToAsr, {
    sessionId: dictSessionId,
  });
  dictRecording = true;
  useDictationStore.setState({ recording: true });
  pushDictLog(
    `开始录音 session=${dictSessionId.slice(0, 8)}（后端麦克风就绪 ${micMs}ms，补发 ${attach.flushedChunks || 0} 块）`,
  );

  playCue("start");
  cmdSilent(CMD.setIndicatorState, { state: "recording" });
  cmdSilent(CMD.setIndicatorText, { text: "" });
  setDictationStatus("正在聆听…（再次按快捷键停止并注入）", "ok");
}

async function stopDictationAndInject() {
  if (!dictRecording) return;
  dictRecording = false;
  useDictationStore.setState({ recording: false });
  try {
    await cmd(CMD.pauseBackendMic);
  } catch (error) {
    pushDictLog(`暂停后端采集失败，仍继续 finish：${String(error)}`);
  }
  scheduleMicShutdown(pushDictLog);
  const session = dictSessionId;

  const durationSec = ((Date.now() - dictStartedAt) / 1000).toFixed(1);
  pushDictLog(`停止录音：时长≈${durationSec}s，已累计 ${dictCommitted.length} 字`);
  cmdSilent(CMD.setIndicatorState, { state: "processing" });
  pushIndicatorText(dictCommitted + dictSegment, { force: true });
  setDictationStatus("识别中，正在等待完整文本…");
  dictAwaitingFinal = true;

  if (!session) {
    pushDictLog("停止时没有有效 ASR 会话，使用已累计文本收尾。");
    scheduleDictFinalize(800);
    return;
  }

  try {
    pushDictLog("已停止后端采集，发送 finish，等待最终结果…");
    await cmd(CMD.asrStreamFinish, { sessionId: session });
  } catch (error) {
    pushDictLog(`停止阶段出错：${String(error)}`);
  }

  scheduleDictFinalize(8000);
}

export async function onCancelKey() {
  await cancelDictation();
}

export async function cancelDictation() {
  if (!dictRecording && !dictAwaitingFinal && !dictSessionId) return;
  const session = dictSessionId;
  dictRecording = false;
  dictAwaitingFinal = false;
  dictFinalized = true;
  dictSessionId = null;
  dictCommitted = "";
  dictSegment = "";
  useDictationStore.setState({ recording: false });
  resetIndicatorPreview();
  if (dictFinalizeTimer) {
    clearTimeout(dictFinalizeTimer);
    dictFinalizeTimer = null;
  }
  scheduleMicShutdown(pushDictLog);
  cmdSilent(CMD.pauseBackendMic);
  cmdSilent(CMD.setIndicatorState, { state: "hidden" });
  cmdSilent(CMD.setIndicatorText, { text: "" });
  if (session) cmdSilent(CMD.stopAsrStream, { sessionId: session });
  pushDictLog("已按 ESC 取消语音输入，识别文本已丢弃。");
  setDictationStatus(`已取消语音输入，快捷键：${comboLabel()}`);
}

async function finalizeDictation() {
  if (dictFinalized || !dictAwaitingFinal) return;
  dictFinalized = true;
  dictAwaitingFinal = false;
  if (dictFinalizeTimer) {
    clearTimeout(dictFinalizeTimer);
    dictFinalizeTimer = null;
  }
  const text = (dictCommitted + dictSegment).trim();
  pushDictLog(
    `收尾：最终 ${text.length} 字（累计段 ${dictCommitted.length} + 当前段 ${dictSegment.length}），共 ${dictResultCount} 条结果`,
  );
  const session = dictSessionId;
  if (session) {
    await cmdSilent(CMD.stopAsrStream, { sessionId: session });
  }
  dictSessionId = null;
  resetIndicatorPreview();

  if (!text) {
    cmdSilent(CMD.setIndicatorState, { state: "hidden" });
    pushDictLog("最终文本为空。");
    setDictationStatus("未识别到文本。", "err");
    playCue("end");
    return;
  }

  const finalText = await runLocalProcessing(text);
  cmdSilent(CMD.setIndicatorState, { state: "hidden" });
  useDictationStore.setState({ latestText: finalText });
  try {
    pushDictLog(`开始注入（方式=${getInjectMethod()}）…`);
    await cmd(CMD.injectText, { text: finalText, method: getInjectMethod() });
    pushDictLog("注入命令已执行完成。");
    setDictationStatus(
      `已注入：${finalText.slice(0, 40)}${finalText.length > 40 ? "…" : ""}`,
      "ok",
    );
  } catch (error) {
    pushDictLog(`注入失败：${String(error)}`);
    setDictationStatus(`注入失败：${String(error)}`, "err");
  }
  playCue("end");
}

export async function toggleDictation() {
  if (dictBusy) return;
  dictBusy = true;
  try {
    if (!dictRecording) await startDictation();
    else await stopDictationAndInject();
  } catch (error) {
    dictRecording = false;
    dictAwaitingFinal = false;
    useDictationStore.setState({ recording: false });
    await shutdownMic();
    cmdSilent(CMD.setIndicatorState, { state: "hidden" });
    if (dictSessionId) {
      cmdSilent(CMD.stopAsrStream, { sessionId: dictSessionId });
      dictSessionId = null;
    }
    setDictationStatus(`语音输入出错：${String(error)}`, "err");
  } finally {
    setTimeout(() => {
      dictBusy = false;
    }, 350);
  }
}

export function handleDictAsrEvent(data: {
  session_id?: string;
  kind?: string;
  payload?: { text?: string; final?: boolean };
}): boolean {
  if (!data.session_id || data.session_id !== dictSessionId) return false;
  if (data.kind === "result") {
    const text = data.payload?.text || "";
    if (text) {
      dictSegment = text;
      pushIndicatorText(dictCommitted + dictSegment);
    }
    if (data.payload?.final && dictSegment) {
      dictCommitted += dictSegment;
      dictSegment = "";
    }
    dictResultCount += 1;
    if (dictResultCount <= 3 || dictResultCount % 10 === 0) {
      pushDictLog(`结果 #${dictResultCount}：当前段 ${text.length} 字`);
    }
    if (dictAwaitingFinal) scheduleDictFinalize(2000);
  } else if (data.kind === "finish" || data.kind === "finish_timeout") {
    pushDictLog(
      data.kind === "finish_timeout"
        ? `等待 finish 超时，使用当前文本收尾（当前段 ${dictSegment.length} 字）`
        : `收到 finish（当前段 ${dictSegment.length} 字）`,
    );
    handleDictSegmentEnd(data.session_id);
  } else if (data.kind === "ended" || data.kind === "closed") {
    pushDictLog(`连接 ${data.kind}：${compactLogJson(data.payload)}`);
    if (dictAwaitingFinal) handleDictSegmentEnd(data.session_id);
  } else if (data.kind === "error") {
    pushDictLog(`ASR 错误：${compactLogJson(data.payload)}`);
    if (dictAwaitingFinal) handleDictSegmentEnd(data.session_id);
    else setDictationStatus(`ASR 错误：${compactLogJson(data.payload)}`, "err");
  } else if (data.kind === "opened") {
    pushDictLog("ASR 连接已打开");
  }
  return true;
}

export function handleShortcutError(payload: { key_code?: string; message?: string }) {
  setDictationStatus(
    `快捷键注册失败（${payload.key_code || "?"}）：${payload.message || "未知错误"}`,
    "err",
  );
}

export function shutdownDictationMic() {
  shutdownMic();
}
