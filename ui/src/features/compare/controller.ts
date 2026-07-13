import type { UnlistenFn } from "@tauri-apps/api/event";
import { convertFileSrc } from "@tauri-apps/api/core";
import { CMD, EVT, cmd, cmdSilent, on } from "@/lib/tauri";
import { base64ToFloat32, float32ToBase64 } from "@/lib/audio-dsp";
import { useDictPrefs } from "@/store/useDictPrefs";
import { useProviderStore } from "@/store/useProviderStore";
import { ensureMic, getBackendMicSampleRate, shutdownMic } from "@/features/audio/micSession";
import { ensureProviderReady } from "@/features/transcription/controller";
import { modelKind, type CompareModelKind } from "@/features/compare/models";
import { useCompareStore } from "@/store/useCompareStore";
import type { TranscriptionEventPayload } from "@/store/useTranscriptionStore";

interface ActiveCell {
  index: number;
  modelValue: string;
  kind: CompareModelKind;
}

interface AsrStreamEventPayload {
  session_id?: string;
  kind?: string;
  payload?: { text?: string; final?: boolean; message?: string };
}

let activeCells: ActiveCell[] = [];
const sessionByIndex = new Map<number, string>();
const finishRequested = new Set<number>();
const jobByIndex = new Map<number, string>();
const committedByIndex = new Map<number, string>();

let asrUnlisten: UnlistenFn | null = null;
let transcriptionUnlisten: UnlistenFn | null = null;
let micRawUnlisten: UnlistenFn | null = null;
let recordedChunks: Float32Array[] = [];
let recordedSampleRate = 48000;

let audioEl: HTMLAudioElement | null = null;
let feederTimer: ReturnType<typeof setInterval> | null = null;
let pcmSamples: Float32Array | null = null;
let pcmSampleRate = 16000;
let lastSentIndex = 0;

function pushLog(message: string) {
  if (useDictPrefs.getState().prefs.debugLog) console.log(`[compare] ${message}`);
}

function buildActiveCells(): ActiveCell[] {
  const { cellModels } = useCompareStore.getState().prefs;
  const cells: ActiveCell[] = [];
  cellModels.forEach((value, index) => {
    if (!value) return;
    const kind = modelKind(value);
    if (!kind) return;
    cells.push({ index, modelValue: value, kind });
  });
  return cells;
}

function buildFileModelParams(model: string) {
  return { model, languageHints: [] as string[], diarizationEnabled: false, speakerCount: null };
}

/** 等后端麦克风原始音频 channel 真正关闭，保证尾块已经通过事件送达前端。参照 features/audio/lab.ts 的同名逻辑。 */
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

function stopListeners() {
  asrUnlisten?.();
  asrUnlisten = null;
  transcriptionUnlisten?.();
  transcriptionUnlisten = null;
}

/** 某个格子进入终态（done/error）后调用：所有 session/job 都清空时，把整体阶段收回 idle。 */
function checkAllSettled() {
  const store = useCompareStore.getState();
  if (store.phase !== "finalizing") return;
  if (sessionByIndex.size === 0 && jobByIndex.size === 0) {
    stopListeners();
    store.setRuntime({ phase: "idle" });
  }
}

async function openRealtimeSession(cell: ActiveCell, sampleRate: number) {
  const session = await cmd<{ session_id: string }>(CMD.startAsrStream, {
    providerId: useProviderStore.getState().effective("asr"),
    modelOverride: cell.modelValue,
    sampleRate,
    params: useDictPrefs.getState().dspParams(),
  });
  sessionByIndex.set(cell.index, session.session_id);
  useCompareStore.getState().patchCellRuntime(cell.index, { status: "connecting" });
}

function handleCompareAsrEvent(payload: AsrStreamEventPayload) {
  if (!payload.session_id) return;
  const entry = [...sessionByIndex.entries()].find(([, sessionId]) => sessionId === payload.session_id);
  if (!entry) return;
  const [index] = entry;
  const store = useCompareStore.getState();

  if (payload.kind === "result") {
    const text = payload.payload?.text || "";
    const committed = committedByIndex.get(index) || "";
    if (text) {
      const display = [committed, text].filter(Boolean).join(committed && text ? "\n" : "");
      store.patchCellRuntime(index, { status: "streaming", text: display });
    }
    if (payload.payload?.final && text.trim()) {
      committedByIndex.set(index, [committed, text.trim()].filter(Boolean).join("\n"));
    }
  } else if (payload.kind === "error") {
    pushLog(`session ${payload.session_id} 出错：${payload.payload?.message || "未知错误"}`);
  } else if (payload.kind === "ended") {
    const expected = finishRequested.has(index);
    store.patchCellRuntime(
      index,
      expected ? { status: "done" } : { status: "error", errorMessage: "连接意外断开" },
    );
    cmdSilent(CMD.stopAsrStream, { sessionId: payload.session_id });
    sessionByIndex.delete(index);
    finishRequested.delete(index);
    checkAllSettled();
  }
}

function handleCompareTranscriptionEvent(payload: TranscriptionEventPayload) {
  if (!payload.jobId) return;
  const entry = [...jobByIndex.entries()].find(([, jobId]) => jobId === payload.jobId);
  if (!entry) return;
  const [index] = entry;
  const store = useCompareStore.getState();

  if (payload.stage === "uploading") {
    store.patchCellRuntime(index, { status: "uploading" });
    return;
  }
  if (payload.stage === "submitted" || payload.stage === "polling") {
    store.patchCellRuntime(index, { status: "recognizing" });
    return;
  }
  if (payload.stage === "completed") {
    const text = (payload.result?.transcripts || [])
      .map((transcript) => transcript.text)
      .filter(Boolean)
      .join("\n");
    store.patchCellRuntime(index, { status: "done", text });
    jobByIndex.delete(index);
    checkAllSettled();
    return;
  }
  if (payload.stage === "error") {
    store.patchCellRuntime(index, {
      status: "error",
      errorMessage: payload.cancelled ? "已取消" : payload.message || "识别失败",
    });
    jobByIndex.delete(index);
    checkAllSettled();
  }
}

async function startRecordCompare() {
  const store = useCompareStore.getState();
  const realtimeCells = activeCells.filter((cell) => cell.kind === "realtime");
  const fileCells = activeCells.filter((cell) => cell.kind === "file");

  await ensureMic(pushLog);
  recordedSampleRate = getBackendMicSampleRate() || 48000;
  recordedChunks = [];

  for (const cell of realtimeCells) {
    await openRealtimeSession(cell, recordedSampleRate);
  }
  for (const cell of fileCells) {
    store.patchCellRuntime(cell.index, { status: "queued" });
  }

  asrUnlisten = await on<AsrStreamEventPayload>(EVT.asrStreamEvent, handleCompareAsrEvent);
  micRawUnlisten = await on<string>(EVT.backendMicRawChunk, (base64) => {
    recordedChunks.push(base64ToFloat32(base64));
    for (const sessionId of sessionByIndex.values()) {
      cmdSilent(CMD.asrStreamPushF32Chunk, { sessionId, audioBase64: base64 });
    }
  });
  await cmd(CMD.attachBackendMicRawCapture);

  store.setRuntime({ phase: "recording" });
}

export async function stopRecording() {
  const store = useCompareStore.getState();
  if (store.phase !== "recording") return;

  const ended = waitForMicCaptureEnded();
  await cmdSilent(CMD.pauseBackendMic);
  await ended;
  micRawUnlisten?.();
  micRawUnlisten = null;
  await shutdownMic();

  let total = 0;
  for (const chunk of recordedChunks) total += chunk.length;
  const merged = new Float32Array(total);
  let offset = 0;
  for (const chunk of recordedChunks) {
    merged.set(chunk, offset);
    offset += chunk.length;
  }
  recordedChunks = [];

  const fileCells = activeCells.filter((cell) => cell.kind === "file");
  let wavPath = "";
  if (fileCells.length > 0) {
    if (total === 0) {
      for (const cell of fileCells) {
        store.patchCellRuntime(cell.index, { status: "error", errorMessage: "未录到音频" });
      }
    } else {
      try {
        wavPath = await cmd<string>(CMD.encodeMonoWavFile, {
          samplesBase64: float32ToBase64(merged),
          sampleRate: recordedSampleRate,
        });
      } catch (error) {
        for (const cell of fileCells) {
          store.patchCellRuntime(cell.index, { status: "error", errorMessage: String(error || "保存录音失败") });
        }
      }
    }
  }

  for (const [index, sessionId] of sessionByIndex) {
    finishRequested.add(index);
    cmdSilent(CMD.asrStreamFinish, { sessionId });
    store.patchCellRuntime(index, { status: "recognizing" });
  }

  if (wavPath) {
    if (!transcriptionUnlisten) {
      transcriptionUnlisten = await on<TranscriptionEventPayload>(
        EVT.transcriptionEvent,
        handleCompareTranscriptionEvent,
      );
    }
    for (const cell of fileCells) {
      try {
        store.patchCellRuntime(cell.index, { status: "uploading" });
        const response = await cmd<{ jobId: string }>(CMD.transcriptionStart, {
          filePath: wavPath,
          params: buildFileModelParams(cell.modelValue),
        });
        jobByIndex.set(cell.index, response.jobId);
      } catch (error) {
        store.patchCellRuntime(cell.index, { status: "error", errorMessage: String(error || "识别启动失败") });
      }
    }
  }

  store.setRuntime({ phase: "finalizing" });
  checkAllSettled();
}

function feedPcmToRealtimeSessions() {
  if (!pcmSamples || !audioEl) return;
  const targetIndex = Math.min(pcmSamples.length, Math.floor(audioEl.currentTime * pcmSampleRate));
  if (targetIndex > lastSentIndex) {
    const slice = new Float32Array(pcmSamples.subarray(lastSentIndex, targetIndex));
    const base64 = float32ToBase64(slice);
    for (const sessionId of sessionByIndex.values()) {
      cmdSilent(CMD.asrStreamPushF32Chunk, { sessionId, audioBase64: base64 });
    }
    lastSentIndex = targetIndex;
  }
  useCompareStore.getState().setRuntime({
    playbackProgress: {
      currentMs: Math.round(audioEl.currentTime * 1000),
      durationMs: Math.round((audioEl.duration || 0) * 1000),
    },
  });
}

async function finalizeUploadRealtime() {
  if (feederTimer) {
    clearInterval(feederTimer);
    feederTimer = null;
  }
  audioEl?.pause();
  const store = useCompareStore.getState();
  for (const [index, sessionId] of sessionByIndex) {
    finishRequested.add(index);
    cmdSilent(CMD.asrStreamFinish, { sessionId });
    store.patchCellRuntime(index, { status: "recognizing" });
  }
  store.setRuntime({ phase: "finalizing" });
  checkAllSettled();
}

async function startUploadCompare() {
  const store = useCompareStore.getState();
  const file = store.selectedFile;
  if (!file) return;
  const realtimeCells = activeCells.filter((cell) => cell.kind === "realtime");
  const fileCells = activeCells.filter((cell) => cell.kind === "file");

  if (fileCells.length > 0) {
    transcriptionUnlisten = await on<TranscriptionEventPayload>(
      EVT.transcriptionEvent,
      handleCompareTranscriptionEvent,
    );
    for (const cell of fileCells) {
      try {
        store.patchCellRuntime(cell.index, { status: "uploading" });
        const response = await cmd<{ jobId: string }>(CMD.transcriptionStart, {
          filePath: file.path,
          params: buildFileModelParams(cell.modelValue),
        });
        jobByIndex.set(cell.index, response.jobId);
      } catch (error) {
        store.patchCellRuntime(cell.index, { status: "error", errorMessage: String(error || "识别启动失败") });
      }
    }
  }

  if (realtimeCells.length > 0) {
    const decoded = await cmd<{ sampleRate: number; samplesBase64: string }>(CMD.decodeAudioFilePcm, {
      filePath: file.path,
    });
    pcmSampleRate = decoded.sampleRate || 16000;
    pcmSamples = base64ToFloat32(decoded.samplesBase64);
    lastSentIndex = 0;

    for (const cell of realtimeCells) {
      await openRealtimeSession(cell, pcmSampleRate);
    }
    asrUnlisten = await on<AsrStreamEventPayload>(EVT.asrStreamEvent, handleCompareAsrEvent);

    audioEl = new Audio(convertFileSrc(file.path));
    audioEl.addEventListener("loadedmetadata", () => {
      useCompareStore.getState().setRuntime({
        playbackProgress: { currentMs: 0, durationMs: Math.round((audioEl?.duration || 0) * 1000) },
      });
    });
    audioEl.addEventListener("ended", () => {
      void finalizeUploadRealtime();
    });
    feederTimer = setInterval(feedPcmToRealtimeSessions, 100);
    await audioEl.play();
    store.setRuntime({ phase: "playing" });
  } else {
    store.setRuntime({ phase: "finalizing" });
    checkAllSettled();
  }
}

export async function startCompare() {
  const store = useCompareStore.getState();
  if (store.phase !== "idle") return;

  const cells = buildActiveCells();
  if (cells.length === 0) {
    store.setRuntime({ globalError: "请至少选择一个模型" });
    return;
  }
  if (store.prefs.sourceMode === "upload" && !store.selectedFile) {
    store.setRuntime({ globalError: "请先选择音频文件" });
    return;
  }
  if (!(await ensureProviderReady())) {
    store.setRuntime({ globalError: "请先在设置中保存阿里云百炼 API Key" });
    return;
  }

  store.resetRuntime();
  activeCells = cells;
  committedByIndex.clear();
  sessionByIndex.clear();
  finishRequested.clear();
  jobByIndex.clear();

  try {
    if (store.prefs.sourceMode === "record") {
      await startRecordCompare();
    } else {
      await startUploadCompare();
    }
  } catch (error) {
    useCompareStore.getState().setRuntime({ globalError: String(error || "启动对比失败") });
    await hardAbortCompare();
  }
}

async function stopUploadRun() {
  await finalizeUploadRealtime();
  const store = useCompareStore.getState();
  for (const [index, jobId] of jobByIndex) {
    cmdSilent(CMD.transcriptionCancel, { jobId });
    store.patchCellRuntime(index, { status: "error", errorMessage: "已取消" });
  }
  jobByIndex.clear();
  checkAllSettled();
}

export async function stopCompare() {
  const phase = useCompareStore.getState().phase;
  if (phase === "recording") {
    await stopRecording();
  } else if (phase === "playing") {
    await stopUploadRun();
  } else if (phase === "finalizing") {
    const store = useCompareStore.getState();
    for (const [index, jobId] of jobByIndex) {
      cmdSilent(CMD.transcriptionCancel, { jobId });
      store.patchCellRuntime(index, { status: "error", errorMessage: "已取消" });
    }
    jobByIndex.clear();
    checkAllSettled();
  }
}

/** 不做优雅收尾的兜底清理：面板卸载 / 应用关闭时调用，防止孤儿 session/job/麦克风占用。 */
export async function hardAbortCompare() {
  if (feederTimer) {
    clearInterval(feederTimer);
    feederTimer = null;
  }
  if (audioEl) {
    audioEl.pause();
    audioEl.src = "";
    audioEl = null;
  }
  pcmSamples = null;

  await cmdSilent(CMD.pauseBackendMic);
  micRawUnlisten?.();
  micRawUnlisten = null;
  await shutdownMic();
  recordedChunks = [];

  for (const sessionId of sessionByIndex.values()) {
    cmdSilent(CMD.stopAsrStream, { sessionId });
  }
  sessionByIndex.clear();
  finishRequested.clear();

  for (const jobId of jobByIndex.values()) {
    cmdSilent(CMD.transcriptionCancel, { jobId });
  }
  jobByIndex.clear();

  stopListeners();
  activeCells = [];
  committedByIndex.clear();

  useCompareStore.getState().setRuntime({ phase: "idle" });
}
