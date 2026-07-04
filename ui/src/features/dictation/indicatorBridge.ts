import { CMD, EVT, cmdSilent, emitEvent } from "@/lib/tauri";
import {
  DICT_INDICATOR_INTERVAL_MS,
  DICT_INDICATOR_WINDOW_KEEP,
  DICT_INDICATOR_WINDOW_MAX,
} from "@/lib/constants";

// ---- 悬浮窗文本节流 ----
let dictIndicatorTextTimer = 0;
let dictIndicatorDisplayText = "";
let dictIndicatorPending: string | null = null;
let dictIndicatorPendingFade = false;
let dictIndicatorTrailTimer: ReturnType<typeof setTimeout> | 0 = 0;
let dictIndicatorWindowStart = 0;

function indicatorPreviewText(text: string): string {
  const value = String(text || "");
  const len = value.length;
  if (dictIndicatorWindowStart > len) dictIndicatorWindowStart = 0;
  if (len - dictIndicatorWindowStart > DICT_INDICATOR_WINDOW_MAX) {
    dictIndicatorWindowStart = len - DICT_INDICATOR_WINDOW_KEEP;
  }
  return dictIndicatorWindowStart > 0
    ? value.slice(dictIndicatorWindowStart).replace(/^\s+/, "")
    : value;
}

export function resetIndicatorPreview() {
  if (dictIndicatorTrailTimer) {
    clearTimeout(dictIndicatorTrailTimer);
    dictIndicatorTrailTimer = 0;
  }
  dictIndicatorPending = null;
  dictIndicatorPendingFade = false;
  dictIndicatorWindowStart = 0;
  dictIndicatorTextTimer = 0;
  dictIndicatorDisplayText = "";
  emitEvent(EVT.indicatorWaveform, { active: false, level: 0, peaks: [] }).catch(() => {});
}

function flushIndicatorText() {
  dictIndicatorTrailTimer = 0;
  if (dictIndicatorPending === null) return;
  const preview = dictIndicatorPending;
  const fade = dictIndicatorPendingFade;
  dictIndicatorPending = null;
  dictIndicatorPendingFade = false;
  if (preview === dictIndicatorDisplayText && !fade) return;
  dictIndicatorTextTimer = Date.now();
  dictIndicatorDisplayText = preview;
  cmdSilent(CMD.setIndicatorText, { text: preview, fade });
}

export function pushIndicatorText(text: string, options: { force?: boolean; fade?: boolean } = {}) {
  const preview = indicatorPreviewText(text);
  if (preview === dictIndicatorDisplayText && dictIndicatorPending === null && !options.fade) return;
  dictIndicatorPending = preview;
  if (options.fade) dictIndicatorPendingFade = true;

  const elapsed = Date.now() - dictIndicatorTextTimer;
  if (options.force || elapsed >= DICT_INDICATOR_INTERVAL_MS) {
    if (dictIndicatorTrailTimer) {
      clearTimeout(dictIndicatorTrailTimer);
      dictIndicatorTrailTimer = 0;
    }
    flushIndicatorText();
  } else if (!dictIndicatorTrailTimer) {
    dictIndicatorTrailTimer = setTimeout(flushIndicatorText, DICT_INDICATOR_INTERVAL_MS - elapsed);
  }
}

export function pushIndicatorWaveform(level: number, active = true, peaks: number[] = []) {
  emitEvent(EVT.indicatorWaveform, {
    active,
    level: Math.max(0, Math.min(1, Number(level) || 0)),
    peaks: peaks.map((value) => Math.max(0, Math.min(1, Number(value) || 0))),
  }).catch(() => {});
}
