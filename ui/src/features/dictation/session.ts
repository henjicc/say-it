import type { UnlistenFn } from "@tauri-apps/api/event";
import { useDictPrefs } from "@/store/useDictPrefs";
import { useDictationStore } from "@/store/useDictationStore";

export type Tone = "" | "ok" | "err";

export const DICTATION_INDICATOR_LAYOUT = { width: 460, height: 188, anchor: "bottom", offsetY: 36 as const };

export const dictSession = {
  sessionId: null as string | null,
  recording: false,
  busy: false,
  awaitingFinal: false,
  committed: "",
  segment: "",
  finalized: false,
  resultCount: 0,
  startedAt: 0,
  finalizeTimer: null as ReturnType<typeof setTimeout> | null,
  mode: null as "realtime" | "file" | null,
  rawUnlisten: null as UnlistenFn | null,
  previewUnlisten: null as UnlistenFn | null,
  silenceTimer: null as ReturnType<typeof setTimeout> | null,
  silenceDisconnecting: false,
  streamStarting: false,
  rawChunks: [] as Float32Array[],
  rawSampleRate: 48000,
  fileJobId: "",
};

export function dspParams() {
  return useDictPrefs.getState().dspParams();
}

export function setDictationStatus(text: string, tone: Tone = "") {
  useDictationStore.setState({ statusText: text, statusTone: tone });
}

export function pushDictLog(message: string) {
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
