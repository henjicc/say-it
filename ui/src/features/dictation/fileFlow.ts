import type { UnlistenFn } from "@tauri-apps/api/event";
import { CMD, EVT, cmd, cmdSilent, on } from "@/lib/tauri";
import { base64ToFloat32, float32ToBase64, measure } from "@/lib/audio-dsp";
import { useDictPrefs } from "@/store/useDictPrefs";
import { useDictationStore } from "@/store/useDictationStore";
import { useProviderStore } from "@/store/useProviderStore";
import type { TranscriptionEventPayload } from "@/store/useTranscriptionStore";
import { playCue } from "@/lib/cues";
import { pushIndicatorWaveform, resetIndicatorPreview } from "./indicatorBridge";
import { ensureMic, getBackendMicSampleRate, scheduleMicShutdown } from "./micSession";
import { dictSession, DICTATION_INDICATOR_LAYOUT, dspParams, pushDictLog, setDictationStatus } from "./session";
import { injectFinalText } from "./inject";

const FILE_WAVEFORM_BUCKETS_PER_CHUNK = 4;

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

async function ensureDictationProviderReady() {
  if (useProviderStore.getState().profiles.length === 0) {
    await useProviderStore.getState().load();
  }
  const state = useProviderStore.getState();
  const providerId = state.effective("asr");
  return !!state.profiles.find((profile) => profile.id === providerId)?.status?.hasApiKey;
}

function buildFileModelParams(model: string) {
  return { model, languageHints: [] as string[], diarizationEnabled: false, speakerCount: null };
}

function mergeRawChunks() {
  let total = 0;
  for (const chunk of dictSession.rawChunks) total += chunk.length;
  const merged = new Float32Array(total);
  let offset = 0;
  for (const chunk of dictSession.rawChunks) {
    merged.set(chunk, offset);
    offset += chunk.length;
  }
  dictSession.rawChunks = [];
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

export async function startFileDictation(model: string) {
  if (!(await ensureDictationProviderReady())) {
    const providerId = useProviderStore.getState().effective("asr");
    const message = providerId
      ? `请先在设置中保存 ${useProviderStore.getState().labelFor(providerId)} API Key`
      : "请先在设置中配置识别供应商 API Key";
    throw new Error(message);
  }

  const t0 = Date.now();
  await ensureMic(pushDictLog);
  const micMs = Date.now() - t0;
  dictSession.rawSampleRate = getBackendMicSampleRate() || 48000;
  dictSession.rawChunks = [];

  dictSession.rawUnlisten = await on<string>(EVT.backendMicRawChunk, (base64) => {
    const samples = base64ToFloat32(base64);
    dictSession.rawChunks.push(samples);
  });
  dictSession.previewUnlisten = await on<{ sampleRate?: number; samplesBase64?: string }>(
    EVT.backendMicPreviewChunk,
    (payload) => {
      // 取消监听前已经进入事件队列的预览块仍可能迟到；切到实时识别后不能再让它重新激活波纹界面。
      if (dictSession.mode !== "file" || !dictSession.recording) return;
      const samples = base64ToFloat32(payload.samplesBase64 || "");
      if (!samples.length) return;
      const { peak } = measure(samples);
      pushIndicatorWaveform(Math.min(1, peak * 1.15), true, summarizeWaveformPeaks(samples));
    },
  );
  const attach = await cmd<{ flushedChunks?: number }>(CMD.attachBackendMicRawCapture, {
    previewParams: dspParams(),
  });

  dictSession.mode = "file";
  dictSession.recording = true;
  useDictationStore.setState({ recording: true });
  pushDictLog(
    `开始非实时录音 model=${model}（后端麦克风就绪 ${micMs}ms，补发 ${attach.flushedChunks || 0} 块）`,
  );

  playCue("start");
  cmdSilent(CMD.setIndicatorLayout, DICTATION_INDICATOR_LAYOUT);
  cmdSilent(CMD.setIndicatorState, { state: "recording" });
  cmdSilent(CMD.setIndicatorText, { text: "" });
  setDictationStatus("正在录音…（非实时）", "ok");
}

export async function stopFileDictationAndRecognize() {
  dictSession.recording = false;
  useDictationStore.setState({ recording: false });
  const ended = waitForMicCaptureEnded();
  try {
    await cmd(CMD.pauseBackendMic);
  } catch (error) {
    pushDictLog(`暂停后端采集失败，仍继续处理录音：${String(error)}`);
  }
  await ended;
  dictSession.rawUnlisten?.();
  dictSession.rawUnlisten = null;
  dictSession.previewUnlisten?.();
  dictSession.previewUnlisten = null;
  scheduleMicShutdown(pushDictLog);

  const samples = mergeRawChunks();
  const durationSec = (samples.length / Math.max(1, dictSession.rawSampleRate)).toFixed(1);
  pushDictLog(`停止非实时录音：时长≈${durationSec}s，样本=${samples.length}`);
  cmdSilent(CMD.setIndicatorState, { state: "processing" });
  cmdSilent(CMD.setIndicatorText, { text: "" });
  pushIndicatorWaveform(0, false);
  setDictationStatus("识别中，正在处理完整录音…");
  dictSession.awaitingFinal = true;

  if (samples.length === 0) {
    dictSession.awaitingFinal = false;
    dictSession.finalized = true;
    cmdSilent(CMD.setIndicatorState, { state: "hidden" });
    setDictationStatus("未录到音频。", "err");
    playCue("end");
    return;
  }

  try {
    const wavPath = await cmd<string>(CMD.encodeMonoWavFile, {
      samplesBase64: float32ToBase64(samples),
      sampleRate: dictSession.rawSampleRate,
    });
    const response = await cmd<{ jobId: string }>(CMD.transcriptionStart, {
      filePath: wavPath,
      params: buildFileModelParams(useDictPrefs.getState().prefs.asrModel),
    });
    dictSession.fileJobId = response.jobId;
    pushDictLog(`非实时识别任务已启动 job=${dictSession.fileJobId.slice(0, 8)}`);
  } catch (error) {
    dictSession.awaitingFinal = false;
    dictSession.finalized = true;
    dictSession.fileJobId = "";
    cmdSilent(CMD.setIndicatorState, { state: "hidden" });
    setDictationStatus(`识别启动失败：${String(error)}`, "err");
    playCue("end");
  }
}

function textFromTranscriptionResult(result: TranscriptionEventPayload["result"]) {
  return (result?.transcripts || [])
    .map((transcript) => transcript.text)
    .filter(Boolean)
    .join("\n")
    .trim();
}

async function finalizeFileDictation(text: string) {
  if (dictSession.finalized || !dictSession.awaitingFinal) return;
  dictSession.finalized = true;
  dictSession.awaitingFinal = false;
  dictSession.fileJobId = "";
  dictSession.mode = null;
  resetIndicatorPreview();
  pushDictLog(`非实时识别完成：最终 ${text.length} 字`);
  await injectFinalText(text);
}

export function handleDictTranscriptionEvent(payload: TranscriptionEventPayload): boolean {
  if (!payload.jobId || payload.jobId !== dictSession.fileJobId) return false;
  if (payload.stage === "uploading") {
    setDictationStatus("正在准备完整录音识别…");
  } else if (payload.stage === "submitted") {
    setDictationStatus("识别任务已提交，正在等待云端处理…");
  } else if (payload.stage === "polling") {
    setDictationStatus(`云端识别中${payload.pollCount ? `（第 ${payload.pollCount} 次查询）` : ""}…`);
  } else if (payload.stage === "completed") {
    void finalizeFileDictation(textFromTranscriptionResult(payload.result));
  } else if (payload.stage === "error") {
    dictSession.awaitingFinal = false;
    dictSession.finalized = true;
    dictSession.fileJobId = "";
    dictSession.mode = null;
    resetIndicatorPreview();
    cmdSilent(CMD.setIndicatorState, { state: "hidden" });
    setDictationStatus(payload.cancelled ? "识别已取消。" : `识别失败：${payload.message || "未知错误"}`, "err");
    playCue("end");
  }
  return true;
}
