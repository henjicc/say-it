import { CMD, EVT, cmd, cmdSilent, on } from "@/lib/tauri";
import { compactLogJson } from "@/lib/format";
import { useDictationStore } from "@/store/useDictationStore";
import { useProviderStore } from "@/store/useProviderStore";
import { playCue } from "@/lib/cues";
import { pushIndicatorText } from "./indicatorBridge";
import { ensureMic, getBackendMicSampleRate, scheduleMicShutdown } from "./micSession";
import { dictSession, DICTATION_INDICATOR_LAYOUT, dspParams, pushDictLog, setDictationStatus } from "./session";
import { finalizeDictation } from "./inject";
import { stopFileDictationAndRecognize } from "./fileFlow";
import { rawChunkRms, silenceDisconnectPrefs } from "@/features/audio/silenceDisconnect";

const MODEL_CALL_DEBUG_ENABLED = false;

let dictationSilenceStartedAt = 0;
let dictationLastLevelLogAt = 0;

async function openDictationAsrStream(model: string) {
  const session = await cmd<{ session_id: string }>(CMD.startAsrStream, {
    providerId: useProviderStore.getState().effective("asr"),
    modelOverride: model,
    sampleRate: getBackendMicSampleRate() || 48000,
    params: dspParams(),
  });
  dictSession.sessionId = session.session_id;
  return cmd<{ flushedChunks?: number }>(CMD.attachBackendMicToAsr, { sessionId: dictSession.sessionId });
}

function clearDictationSilenceTimer() {
  if (dictSession.silenceTimer) {
    clearTimeout(dictSession.silenceTimer);
    dictSession.silenceTimer = null;
  }
  dictationSilenceStartedAt = 0;
}

async function disconnectDictationAsrForSilence() {
  if (!dictSession.recording || !dictSession.sessionId || dictSession.silenceDisconnecting) return;
  dictSession.silenceDisconnecting = true;
  const session = dictSession.sessionId;
  dictSession.sessionId = null;
  await cmdSilent(CMD.stopAsrStream, { sessionId: session });
  dictSession.segment = "";
  pushIndicatorText(dictSession.committed, { force: true });
  setDictationStatus("已因音量低于阈值断开 ASR 流，等待再次说话…", "ok");
  if (MODEL_CALL_DEBUG_ENABLED) console.log(`[model-call] 语音输入 OFF 音量低于阈值断流 session=${session.slice(0, 8)}`);
  cmdSilent(CMD.debugModelCallState, { message: `语音输入 OFF 音量低于阈值断流 session=${session.slice(0, 8)}` });
  pushDictLog(`音量低于阈值达到时长，已断开 ASR 流 session=${session.slice(0, 8)}`);
  dictSession.silenceDisconnecting = false;
}

async function connectDictationAsrOnVoice(model: string) {
  if (!dictSession.recording || dictSession.sessionId || dictSession.streamStarting) return;
  dictSession.streamStarting = true;
  try {
    const attach = await openDictationAsrStream(model);
    const shortSession = dictSession.sessionId?.slice(0, 8) || "?";
    if (MODEL_CALL_DEBUG_ENABLED) console.log(`[model-call] 语音输入 ON session=${shortSession}`);
    cmdSilent(CMD.debugModelCallState, { message: `语音输入 ON session=${shortSession}` });
    pushDictLog(`检测到声音，已连接 ASR session=${dictSession.sessionId?.slice(0, 8)}，补发 ${attach.flushedChunks || 0} 块`);
    setDictationStatus("正在聆听…（静音会自动断开流）", "ok");
  } catch (error) {
    setDictationStatus(`连接 ASR 失败：${String(error)}`, "err");
    pushDictLog(`检测到声音后连接 ASR 失败：${String(error)}`);
  } finally {
    dictSession.streamStarting = false;
  }
}

async function startRealtimeSilenceGate(model: string, micMs: number) {
  const prefs = silenceDisconnectPrefs();
  dictSession.rawUnlisten?.();
  dictSession.rawUnlisten = await on<string>(EVT.backendMicRawChunk, (base64) => {
    const rms = rawChunkRms(base64);
    const now = Date.now();
    if (rms > prefs.dictationSilenceThreshold) {
      clearDictationSilenceTimer();
      if (now - dictationLastLevelLogAt >= 1000) {
        dictationLastLevelLogAt = now;
        cmdSilent(CMD.debugModelCallState, { message: `语音输入 音量=${rms.toFixed(4)} > 阈值 ${prefs.dictationSilenceThreshold.toFixed(4)}，正在调用模型` });
      }
      void connectDictationAsrOnVoice(model);
      return;
    }
    if (dictSession.sessionId) {
      if (!dictationSilenceStartedAt) dictationSilenceStartedAt = now;
      const remainingMs = Math.max(0, prefs.dictationSilenceDisconnectMs - (now - dictationSilenceStartedAt));
      if (now - dictationLastLevelLogAt >= 1000) {
        dictationLastLevelLogAt = now;
        cmdSilent(CMD.debugModelCallState, { message: `语音输入 音量=${rms.toFixed(4)} <= 阈值 ${prefs.dictationSilenceThreshold.toFixed(4)}，约 ${(remainingMs / 1000).toFixed(1)}s 后断流` });
      }
      if (!dictSession.silenceTimer) {
        dictSession.silenceTimer = setTimeout(() => {
          dictSession.silenceTimer = null;
          void disconnectDictationAsrForSilence();
        }, remainingMs);
      }
    } else if (now - dictationLastLevelLogAt >= 1000) {
      dictationLastLevelLogAt = now;
      cmdSilent(CMD.debugModelCallState, { message: `语音输入 音量=${rms.toFixed(4)} <= 阈值 ${prefs.dictationSilenceThreshold.toFixed(4)}，未调用模型` });
    }
  });
  await cmd(CMD.attachBackendMicRawCapture);
  pushDictLog(`开始录音（音量低于阈值断流已开启，后端麦克风就绪 ${micMs}ms，阈值 ${prefs.dictationSilenceThreshold.toFixed(3)}）`);
  setDictationStatus("正在等待声音…（再次按快捷键停止并注入）", "ok");
}

export async function startRealtimeDictation(model: string) {
  const t0 = Date.now();
  await ensureMic(pushDictLog);
  const micMs = Date.now() - t0;

  dictSession.mode = "realtime";
  dictSession.recording = true;
  useDictationStore.setState({ recording: true });
  playCue("start");
  cmdSilent(CMD.setIndicatorLayout, DICTATION_INDICATOR_LAYOUT);
  cmdSilent(CMD.setIndicatorState, { state: "recording" });
  cmdSilent(CMD.setIndicatorText, { text: "" });

  if (silenceDisconnectPrefs().dictationSilenceDisconnectEnabled) {
    cmdSilent(CMD.debugModelCallState, { message: "语音输入 WAIT 本地检测中，未调用模型" });
    await startRealtimeSilenceGate(model, micMs);
    return;
  }

  dictSession.rawUnlisten?.();
  dictSession.rawUnlisten = await on<string>(EVT.backendMicRawChunk, (base64) => {
    const now = Date.now();
    if (now - dictationLastLevelLogAt < 1000) return;
    dictationLastLevelLogAt = now;
    const rms = rawChunkRms(base64);
    cmdSilent(CMD.debugModelCallState, { message: `语音输入 音量=${rms.toFixed(4)}，正在调用模型` });
  });
  await cmd(CMD.attachBackendMicRawCapture);
  const attach = await openDictationAsrStream(model);
  pushDictLog(
    `开始录音 session=${dictSession.sessionId?.slice(0, 8)}（后端麦克风就绪 ${micMs}ms，补发 ${attach.flushedChunks || 0} 块）`,
  );
  setDictationStatus("正在聆听…（再次按快捷键停止并注入）", "ok");
}

function scheduleDictFinalize(delay: number) {
  if (dictSession.finalizeTimer) clearTimeout(dictSession.finalizeTimer);
  dictSession.finalizeTimer = setTimeout(() => finalizeDictation(), delay);
}

function handleDictSegmentEnd(session: string) {
  if (session !== dictSession.sessionId) return;
  finalizeDictation();
}

export async function stopDictationAndInject() {
  if (!dictSession.recording) return;
  if (dictSession.mode === "file") {
    await stopFileDictationAndRecognize();
    return;
  }
  dictSession.recording = false;
  useDictationStore.setState({ recording: false });
  clearDictationSilenceTimer();
  dictSession.rawUnlisten?.();
  dictSession.rawUnlisten = null;
  dictSession.streamStarting = false;
  dictSession.silenceDisconnecting = false;
  try {
    await cmd(CMD.pauseBackendMic);
  } catch (error) {
    pushDictLog(`暂停后端采集失败，仍继续 finish：${String(error)}`);
  }
  scheduleMicShutdown(pushDictLog);
  const session = dictSession.sessionId;

  const durationSec = ((Date.now() - dictSession.startedAt) / 1000).toFixed(1);
  pushDictLog(`停止录音：时长≈${durationSec}s，已累计 ${dictSession.committed.length} 字`);
  cmdSilent(CMD.setIndicatorState, { state: "processing" });
  pushIndicatorText(dictSession.committed + dictSession.segment, { force: true });
  setDictationStatus("识别中，正在等待完整文本…");
  dictSession.awaitingFinal = true;

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

export function handleDictAsrEvent(data: {
  session_id?: string;
  kind?: string;
  payload?: { text?: string; final?: boolean };
}): boolean {
  if (!data.session_id || data.session_id !== dictSession.sessionId) return false;
  if (data.kind === "result") {
    const text = data.payload?.text || "";
    if (text) {
      dictSession.segment = text;
      pushIndicatorText(dictSession.committed + dictSession.segment);
    }
    if (data.payload?.final && dictSession.segment) {
      dictSession.committed += dictSession.segment;
      dictSession.segment = "";
    }
    dictSession.resultCount += 1;
    if (dictSession.resultCount <= 3 || dictSession.resultCount % 10 === 0) {
      pushDictLog(`结果 #${dictSession.resultCount}：当前段 ${text.length} 字`);
    }
    if (dictSession.awaitingFinal) scheduleDictFinalize(2000);
  } else if (data.kind === "finish" || data.kind === "finish_timeout") {
    pushDictLog(
      data.kind === "finish_timeout"
        ? `等待 finish 超时，使用当前文本收尾（当前段 ${dictSession.segment.length} 字）`
        : `收到 finish（当前段 ${dictSession.segment.length} 字）`,
    );
    handleDictSegmentEnd(data.session_id);
  } else if (data.kind === "ended" || data.kind === "closed") {
    pushDictLog(`连接 ${data.kind}：${compactLogJson(data.payload)}`);
    if (silenceDisconnectPrefs().dictationSilenceDisconnectEnabled && dictSession.recording && !dictSession.awaitingFinal) {
      dictSession.sessionId = null;
      clearDictationSilenceTimer();
      setDictationStatus("ASR 流已断开，等待再次说话…", "ok");
      return true;
    }
    if (dictSession.awaitingFinal) handleDictSegmentEnd(data.session_id);
  } else if (data.kind === "error") {
    pushDictLog(`ASR 错误：${compactLogJson(data.payload)}`);
    if (dictSession.awaitingFinal) handleDictSegmentEnd(data.session_id);
    else setDictationStatus(`ASR 错误：${compactLogJson(data.payload)}`, "err");
  } else if (data.kind === "opened") {
    pushDictLog("ASR 连接已打开");
  }
  return true;
}
