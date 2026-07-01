import { CMD, cmdSilent } from "@/lib/tauri";
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
