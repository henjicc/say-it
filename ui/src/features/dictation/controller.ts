import type { UnlistenFn } from "@tauri-apps/api/event";
import { CMD, EVT, cmd, cmdSilent, on } from "@/lib/tauri";
import { compactLogJson } from "@/lib/format";
import { base64ToFloat32, float32ToBase64, measure } from "@/lib/audio-dsp";
import { useDictPrefs } from "@/store/useDictPrefs";
import { useDictationStore } from "@/store/useDictationStore";
import { useProviderStore } from "@/store/useProviderStore";
import type { TranscriptionEventPayload } from "@/store/useTranscriptionStore";
import { isDictationFileModel } from "@/features/asr/modelOptions";
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
import { pushIndicatorText, pushIndicatorWaveform, resetIndicatorPreview } from "./indicatorBridge";
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
let dictMode: "realtime" | "file" | null = null;
let dictRawUnlisten: UnlistenFn | null = null;
let dictPreviewUnlisten: UnlistenFn | null = null;
let dictRawChunks: Float32Array[] = [];
let dictRawSampleRate = 48000;
let dictFileJobId = "";
const FILE_WAVEFORM_BUCKETS_PER_CHUNK = 4;
const DICTATION_INDICATOR_LAYOUT = { width: 460, height: 188, anchor: "bottom", offsetY: 36 as const };

function summarizeWaveformPeaks(samples: Float32Array, bucketCount = FILE_WAVEFORM_BUCKETS_PER_CHUNK) {
  if (!samples.length || bucketCount <= 0) return [];
  const peaks: number[] = [];
  const bucketSize = Math.max(1, Math.floor(samples.length / bucketCount));
  for (let bucket = 0; bucket < bucketCount; bucket += 1) {
    const start = bucket * bucketSize;
    const end = bucket === bucketCount - 1 ? samples.length : Math.min(samples.length, start + bucketSize);
    let peak = 0;
    for (let index = start; index < end; index += 1) {
      peak = Math.max(peak, Math.abs(samples[index] || 0));
    }
    peaks.push(peak);
  }
  return peaks;
}

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

async function ensureDictationProviderReady() {
  if (useProviderStore.getState().profiles.length === 0) {
    await useProviderStore.getState().load();
  }
  return !!useProviderStore
    .getState()
    .profiles.find((profile) => profile.id === "funasr")?.status?.hasApiKey;
}

function buildFileModelParams(model: string) {
  return { model, languageHints: [] as string[], diarizationEnabled: false, speakerCount: null };
}

function mergeRawChunks() {
  let total = 0;
  for (const chunk of dictRawChunks) total += chunk.length;
  const merged = new Float32Array(total);
  let offset = 0;
  for (const chunk of dictRawChunks) {
    merged.set(chunk, offset);
    offset += chunk.length;
  }
  dictRawChunks = [];
  return merged;
}

function waitForMicCaptureEnded(timeoutMs = 1000): Promise<void> {
  return new Promise((resolve) => {
    let done = false;
    let unlisten: UnlistenFn | null = null;
    const finish = () => {
      if (done) return;
      done = true;
      unlisten?.();
      resolve();
    };
    on(EVT.backendMicRawEnded, finish).then((fn) => {
      if (done) fn();
      else unlisten = fn;
    });
    setTimeout(finish, timeoutMs);
  });
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
  dictMode = null;
  dictFileJobId = "";
  dictRawChunks = [];
  resetIndicatorPreview();

  const model = useDictPrefs.getState().prefs.asrModel;
  if (isDictationFileModel(model)) {
    await startFileDictation(model);
    return;
  }

  await startRealtimeDictation(model);
}

async function startRealtimeDictation(model: string) {
  const t0 = Date.now();
  await ensureMic(pushDictLog);
  const micMs = Date.now() - t0;

  const session = await cmd<{ session_id: string }>(CMD.startAsrStream, {
    providerId: useProviderStore.getState().effective("asr"),
    modelOverride: model,
    sampleRate: getBackendMicSampleRate() || 48000,
    params: dspParams(),
  });
  dictSessionId = session.session_id;
  dictMode = "realtime";
  const attach = await cmd<{ flushedChunks?: number }>(CMD.attachBackendMicToAsr, {
    sessionId: dictSessionId,
  });
  dictRecording = true;
  useDictationStore.setState({ recording: true });
  pushDictLog(
    `开始录音 session=${dictSessionId.slice(0, 8)}（后端麦克风就绪 ${micMs}ms，补发 ${attach.flushedChunks || 0} 块）`,
  );

  playCue("start");
  cmdSilent(CMD.setIndicatorLayout, DICTATION_INDICATOR_LAYOUT);
  cmdSilent(CMD.setIndicatorState, { state: "recording" });
  cmdSilent(CMD.setIndicatorText, { text: "" });
  setDictationStatus("正在聆听…（再次按快捷键停止并注入）", "ok");
}

async function startFileDictation(model: string) {
  if (!(await ensureDictationProviderReady())) {
    throw new Error("请先在设置中保存阿里云百炼 API Key");
  }

  const t0 = Date.now();
  await ensureMic(pushDictLog);
  const micMs = Date.now() - t0;
  dictRawSampleRate = getBackendMicSampleRate() || 48000;
  dictRawChunks = [];

  dictRawUnlisten = await on<string>(EVT.backendMicRawChunk, (base64) => {
    const samples = base64ToFloat32(base64);
    dictRawChunks.push(samples);
  });
  dictPreviewUnlisten = await on<{ sampleRate?: number; samplesBase64?: string }>(
    EVT.backendMicPreviewChunk,
    (payload) => {
      const samples = base64ToFloat32(payload.samplesBase64 || "");
      if (!samples.length) return;
      const { peak } = measure(samples);
      pushIndicatorWaveform(Math.min(1, peak * 1.15), true, summarizeWaveformPeaks(samples));
    },
  );
  const attach = await cmd<{ flushedChunks?: number }>(CMD.attachBackendMicRawCapture, {
    previewParams: dspParams(),
  });

  dictMode = "file";
  dictRecording = true;
  useDictationStore.setState({ recording: true });
  pushDictLog(
    `开始非实时录音 model=${model}（后端麦克风就绪 ${micMs}ms，补发 ${attach.flushedChunks || 0} 块）`,
  );

  playCue("start");
  cmdSilent(CMD.setIndicatorLayout, DICTATION_INDICATOR_LAYOUT);
  cmdSilent(CMD.setIndicatorState, { state: "recording" });
  cmdSilent(CMD.setIndicatorText, { text: "" });
  setDictationStatus("正在录音…（停止后识别并注入）", "ok");
}

async function stopDictationAndInject() {
  if (!dictRecording) return;
  if (dictMode === "file") {
    await stopFileDictationAndRecognize();
    return;
  }
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

async function stopFileDictationAndRecognize() {
  dictRecording = false;
  useDictationStore.setState({ recording: false });
  const ended = waitForMicCaptureEnded();
  try {
    await cmd(CMD.pauseBackendMic);
  } catch (error) {
    pushDictLog(`暂停后端采集失败，仍继续处理录音：${String(error)}`);
  }
  await ended;
  dictRawUnlisten?.();
  dictRawUnlisten = null;
  dictPreviewUnlisten?.();
  dictPreviewUnlisten = null;
  scheduleMicShutdown(pushDictLog);

  const samples = mergeRawChunks();
  const durationSec = (samples.length / Math.max(1, dictRawSampleRate)).toFixed(1);
  pushDictLog(`停止非实时录音：时长≈${durationSec}s，样本=${samples.length}`);
  cmdSilent(CMD.setIndicatorState, { state: "processing" });
  cmdSilent(CMD.setIndicatorText, { text: "" });
  pushIndicatorWaveform(0, false);
  setDictationStatus("识别中，正在处理完整录音…");
  dictAwaitingFinal = true;

  if (samples.length === 0) {
    dictAwaitingFinal = false;
    dictFinalized = true;
    cmdSilent(CMD.setIndicatorState, { state: "hidden" });
    setDictationStatus("未录到音频。", "err");
    playCue("end");
    return;
  }

  try {
    const wavPath = await cmd<string>(CMD.encodeMonoWavFile, {
      samplesBase64: float32ToBase64(samples),
      sampleRate: dictRawSampleRate,
    });
    const response = await cmd<{ jobId: string }>(CMD.transcriptionStart, {
      filePath: wavPath,
      params: buildFileModelParams(useDictPrefs.getState().prefs.asrModel),
    });
    dictFileJobId = response.jobId;
    pushDictLog(`非实时识别任务已启动 job=${dictFileJobId.slice(0, 8)}`);
  } catch (error) {
    dictAwaitingFinal = false;
    dictFinalized = true;
    dictFileJobId = "";
    cmdSilent(CMD.setIndicatorState, { state: "hidden" });
    setDictationStatus(`识别启动失败：${String(error)}`, "err");
    playCue("end");
  }
}

export async function onCancelKey() {
  await cancelDictation();
}

export async function cancelDictation() {
  if (!dictRecording && !dictAwaitingFinal && !dictSessionId && !dictFileJobId) return;
  const session = dictSessionId;
  const fileJobId = dictFileJobId;
  dictRecording = false;
  dictAwaitingFinal = false;
  dictFinalized = true;
  dictSessionId = null;
  dictFileJobId = "";
  dictMode = null;
  dictCommitted = "";
  dictSegment = "";
  dictRawChunks = [];
  useDictationStore.setState({ recording: false });
  resetIndicatorPreview();
  if (dictFinalizeTimer) {
    clearTimeout(dictFinalizeTimer);
    dictFinalizeTimer = null;
  }
  scheduleMicShutdown(pushDictLog);
  cmdSilent(CMD.pauseBackendMic);
  dictRawUnlisten?.();
  dictRawUnlisten = null;
  dictPreviewUnlisten?.();
  dictPreviewUnlisten = null;
  cmdSilent(CMD.setIndicatorState, { state: "hidden" });
  cmdSilent(CMD.setIndicatorText, { text: "" });
  if (session) cmdSilent(CMD.stopAsrStream, { sessionId: session });
  if (fileJobId) cmdSilent(CMD.transcriptionCancel, { jobId: fileJobId });
  pushDictLog("已按 ESC 取消语音输入，识别文本已丢弃。");
  setDictationStatus(`已取消语音输入，快捷键：${comboLabel()}`);
}

async function injectFinalText(text: string) {
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
  dictMode = null;
  resetIndicatorPreview();
  await injectFinalText(text);
}

export async function toggleDictation() {
  if (dictBusy) return;
  if (dictAwaitingFinal) {
    setDictationStatus("正在等待识别完成，按 Esc 可取消。");
    return;
  }
  dictBusy = true;
  try {
    if (!dictRecording) await startDictation();
    else await stopDictationAndInject();
  } catch (error) {
    dictRecording = false;
    dictAwaitingFinal = false;
    dictMode = null;
    useDictationStore.setState({ recording: false });
    await shutdownMic();
    dictRawUnlisten?.();
    dictRawUnlisten = null;
    dictPreviewUnlisten?.();
    dictPreviewUnlisten = null;
    dictRawChunks = [];
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

function textFromTranscriptionResult(result: TranscriptionEventPayload["result"]) {
  return (result?.transcripts || [])
    .map((transcript) => transcript.text)
    .filter(Boolean)
    .join("\n")
    .trim();
}

async function finalizeFileDictation(text: string) {
  if (dictFinalized || !dictAwaitingFinal) return;
  dictFinalized = true;
  dictAwaitingFinal = false;
  dictFileJobId = "";
  dictMode = null;
  resetIndicatorPreview();
  pushDictLog(`非实时识别完成：最终 ${text.length} 字`);
  await injectFinalText(text);
}

export function handleDictTranscriptionEvent(payload: TranscriptionEventPayload): boolean {
  if (!payload.jobId || payload.jobId !== dictFileJobId) return false;
  if (payload.stage === "uploading") {
    setDictationStatus("正在准备完整录音识别…");
  } else if (payload.stage === "submitted") {
    setDictationStatus("识别任务已提交，正在等待云端处理…");
  } else if (payload.stage === "polling") {
    setDictationStatus(`云端识别中${payload.pollCount ? `（第 ${payload.pollCount} 次查询）` : ""}…`);
  } else if (payload.stage === "completed") {
    void finalizeFileDictation(textFromTranscriptionResult(payload.result));
  } else if (payload.stage === "error") {
    dictAwaitingFinal = false;
    dictFinalized = true;
    dictFileJobId = "";
    dictMode = null;
    resetIndicatorPreview();
    cmdSilent(CMD.setIndicatorState, { state: "hidden" });
    setDictationStatus(payload.cancelled ? "识别已取消。" : `识别失败：${payload.message || "未知错误"}`, "err");
    playCue("end");
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
  if (dictFileJobId) {
    cmdSilent(CMD.transcriptionCancel, { jobId: dictFileJobId });
    dictFileJobId = "";
  }
  dictRawUnlisten?.();
  dictRawUnlisten = null;
  dictPreviewUnlisten?.();
  dictPreviewUnlisten = null;
  dictRawChunks = [];
  dictMode = null;
  shutdownMic();
}
