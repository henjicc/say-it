import { CMD, cmd } from "@/lib/tauri";
import {
  useDictationStore,
  type ActiveAppContextSummary,
} from "@/store/useDictationStore";
import {
  isCapturing,
  setInjectMethod,
  setPressHoldMode,
  setMainShortcut,
  loadDictationSettings,
  handleCaptureLockKey,
  configureHotkeys,
} from "./hotkeys";

export {
  isCapturing,
  setInjectMethod,
  setPressHoldMode,
  setMainShortcut,
  loadDictationSettings,
  handleCaptureLockKey,
} from "./hotkeys";

type RuntimePayload = {
  phase?: "idle" | "waitingForVoice" | "recording" | "finishing" | "processingFile" | "injecting" | "failed";
  recording?: boolean;
  text?: string;
  error?: string | null;
  activeAppContext?: ActiveAppContextSummary | null;
};

const labels: Record<string, string> = {
  idle: "速记就绪",
  waitingForVoice: "正在等待声音…（再次按快捷键停止并注入）",
  recording: "正在聆听…（再次按快捷键停止并注入）",
  finishing: "识别中，正在等待完整文本…",
  processingFile: "识别中，正在处理完整录音…",
  injecting: "识别完成，正在注入…",
  failed: "语音输入出错",
};

configureHotkeys({
  setStatus: (statusText, statusTone = "") => useDictationStore.setState({ statusText, statusTone }),
});

export function applyDictationRuntime(payload: RuntimePayload) {
  const phase = payload.phase || "idle";
  const error = payload.error || "";
  useDictationStore.setState({
    recording: !!payload.recording,
    latestText: payload.text || useDictationStore.getState().latestText,
    statusText: error ? `语音输入出错：${error}` : labels[phase] || labels.idle,
    statusTone: error || phase === "failed" ? "err" : phase === "idle" ? "" : "ok",
    activeAppContext: payload.activeAppContext ?? useDictationStore.getState().activeAppContext,
  });
}

export async function loadDictationRuntime() {
  const runtime = await cmd<RuntimePayload>(CMD.getDictationRuntime);
  applyDictationRuntime({ ...runtime, recording: runtime.phase === "recording" || runtime.phase === "waitingForVoice" });
}

async function invokeRuntime(command: string) {
  try { await cmd(command); }
  catch (error) { useDictationStore.setState({ statusText: `语音输入出错：${String(error)}`, statusTone: "err" }); }
}
export async function toggleDictation() { await invokeRuntime(CMD.dictationToggle); }
export async function startDictationByShortcut() { await cmd(CMD.dictationStart); }
export async function stopDictationByShortcut() { await cmd(CMD.dictationStop); }
export async function onCancelKey() { await cmd(CMD.dictationCancel); }
export async function cancelDictation() { await cmd(CMD.dictationCancel); }
export function shutdownDictationMic() { /* 生命周期由 Rust 服务持有，不随 WebView 卸载。 */ }
export function clearDictLog() { useDictationStore.setState({ log: "" }); }
export function handleShortcutError(payload: { key_code?: string; message?: string }) {
  useDictationStore.setState({ statusText: `快捷键注册失败（${payload.key_code || "?"}）：${payload.message || "未知错误"}`, statusTone: "err" });
}
