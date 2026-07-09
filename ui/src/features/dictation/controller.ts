import { CMD, cmdSilent } from "@/lib/tauri";
import { useDictPrefs } from "@/store/useDictPrefs";
import { useDictationStore } from "@/store/useDictationStore";
import { isDictationFileModel } from "@/features/asr/modelOptions";
import {
  configureHotkeys,
  comboLabel,
  isCapturing,
  startShortcutCapture,
  setInjectMethod,
  setPressHoldMode,
  handleForwardedKeydown,
  handleForwardedKeyup,
  installFocusHotkeyFallback,
  loadDictationSettings,
  handleCaptureLockKey,
} from "./hotkeys";
import { resetIndicatorPreview } from "./indicatorBridge";
import { clearMicShutdownTimer, scheduleMicShutdown, shutdownMic } from "./micSession";
import { dictSession, clearDictLog, pushDictLog, setDictationStatus } from "./session";
import { startFileDictation, handleDictTranscriptionEvent } from "./fileFlow";
import { startRealtimeDictation, stopDictationAndInject, handleDictAsrEvent } from "./realtimeFlow";

export {
  startShortcutCapture,
  clearShortcut,
  isCapturing,
  setInjectMethod,
  setPressHoldMode,
  handleForwardedKeydown,
  handleForwardedKeyup,
  installFocusHotkeyFallback,
  loadDictationSettings,
  handleCaptureLockKey,
} from "./hotkeys";
export { clearDictLog } from "./session";
export { handleDictAsrEvent } from "./realtimeFlow";
export { handleDictTranscriptionEvent } from "./fileFlow";

configureHotkeys({
  setStatus: setDictationStatus,
  getRecording: () => useDictationStore.getState().recording,
  isAssistantActive: () => false,
  toggleDictation: () => toggleDictation(),
  startDictation: () => startDictationByShortcut(),
  stopDictation: () => stopDictationByShortcut(),
  onCancelKey: () => onCancelKey(),
});

async function startDictation() {
  if (dictSession.recording) return;
  clearMicShutdownTimer();
  dictSession.committed = "";
  dictSession.segment = "";
  dictSession.finalized = false;
  dictSession.awaitingFinal = false;
  dictSession.resultCount = 0;
  dictSession.startedAt = Date.now();
  dictSession.mode = null;
  dictSession.fileJobId = "";
  dictSession.rawChunks = [];
  resetIndicatorPreview();

  const model = useDictPrefs.getState().prefs.asrModel;
  if (isDictationFileModel(model)) {
    await startFileDictation(model);
    return;
  }

  await startRealtimeDictation(model);
}

export async function onCancelKey() {
  await cancelDictation();
}

export async function cancelDictation() {
  if (!dictSession.recording && !dictSession.awaitingFinal && !dictSession.sessionId && !dictSession.fileJobId) return;
  const session = dictSession.sessionId;
  const fileJobId = dictSession.fileJobId;
  dictSession.recording = false;
  dictSession.awaitingFinal = false;
  dictSession.finalized = true;
  dictSession.sessionId = null;
  dictSession.fileJobId = "";
  dictSession.mode = null;
  dictSession.committed = "";
  dictSession.segment = "";
  dictSession.rawChunks = [];
  useDictationStore.setState({ recording: false });
  resetIndicatorPreview();
  if (dictSession.finalizeTimer) {
    clearTimeout(dictSession.finalizeTimer);
    dictSession.finalizeTimer = null;
  }
  scheduleMicShutdown(pushDictLog);
  cmdSilent(CMD.pauseBackendMic);
  dictSession.rawUnlisten?.();
  dictSession.rawUnlisten = null;
  dictSession.previewUnlisten?.();
  dictSession.previewUnlisten = null;
  cmdSilent(CMD.setIndicatorState, { state: "hidden" });
  cmdSilent(CMD.setIndicatorText, { text: "" });
  if (session) cmdSilent(CMD.stopAsrStream, { sessionId: session });
  if (fileJobId) cmdSilent(CMD.transcriptionCancel, { jobId: fileJobId });
  pushDictLog("已按 ESC 取消语音输入，识别文本已丢弃。");
  setDictationStatus(`已取消语音输入，快捷键：${comboLabel()}`);
}

async function waitForShortcutBusy() {
  while (dictSession.busy) {
    await new Promise((resolve) => window.setTimeout(resolve, 20));
  }
}

export async function startDictationByShortcut() {
  if (dictSession.busy || dictSession.recording || dictSession.awaitingFinal) return;
  dictSession.busy = true;
  try {
    await startDictation();
  } catch (error) {
    dictSession.recording = false;
    dictSession.awaitingFinal = false;
    dictSession.mode = null;
    useDictationStore.setState({ recording: false });
    await shutdownMic();
    dictSession.rawUnlisten?.();
    dictSession.rawUnlisten = null;
    dictSession.previewUnlisten?.();
    dictSession.previewUnlisten = null;
    dictSession.rawChunks = [];
    cmdSilent(CMD.setIndicatorState, { state: "hidden" });
    if (dictSession.sessionId) {
      cmdSilent(CMD.stopAsrStream, { sessionId: dictSession.sessionId });
      dictSession.sessionId = null;
    }
    setDictationStatus(`语音输入出错：${String(error)}`, "err");
  } finally {
    dictSession.busy = false;
  }
}

export async function stopDictationByShortcut() {
  if (!dictSession.recording && !dictSession.busy) return;
  await waitForShortcutBusy();
  if (dictSession.recording) await toggleDictation();
}

export async function toggleDictation() {
  if (dictSession.busy) return;
  if (dictSession.awaitingFinal) {
    setDictationStatus("正在等待识别完成，按 Esc 可取消。");
    return;
  }
  dictSession.busy = true;
  try {
    if (!dictSession.recording) await startDictation();
    else await stopDictationAndInject();
  } catch (error) {
    dictSession.recording = false;
    dictSession.awaitingFinal = false;
    dictSession.mode = null;
    useDictationStore.setState({ recording: false });
    await shutdownMic();
    dictSession.rawUnlisten?.();
    dictSession.rawUnlisten = null;
    dictSession.previewUnlisten?.();
    dictSession.previewUnlisten = null;
    dictSession.rawChunks = [];
    cmdSilent(CMD.setIndicatorState, { state: "hidden" });
    if (dictSession.sessionId) {
      cmdSilent(CMD.stopAsrStream, { sessionId: dictSession.sessionId });
      dictSession.sessionId = null;
    }
    setDictationStatus(`语音输入出错：${String(error)}`, "err");
  } finally {
    setTimeout(() => {
      dictSession.busy = false;
    }, 350);
  }
}

export function handleShortcutError(payload: { key_code?: string; message?: string }) {
  setDictationStatus(
    `快捷键注册失败（${payload.key_code || "?"}）：${payload.message || "未知错误"}`,
    "err",
  );
}

export function shutdownDictationMic() {
  if (dictSession.fileJobId) {
    cmdSilent(CMD.transcriptionCancel, { jobId: dictSession.fileJobId });
    dictSession.fileJobId = "";
  }
  dictSession.rawUnlisten?.();
  dictSession.rawUnlisten = null;
  dictSession.previewUnlisten?.();
  dictSession.previewUnlisten = null;
  dictSession.rawChunks = [];
  dictSession.mode = null;
  shutdownMic();
}
