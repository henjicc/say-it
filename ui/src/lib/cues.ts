// 音频提示音（开始/结束 beep）。移植自旧 app.js 的 cue 逻辑。
import { useDictPrefs } from "@/store/useDictPrefs";

let cueCtx: AudioContext | null = null;

function getCueCtx(): AudioContext {
  if (!cueCtx || cueCtx.state === "closed") cueCtx = new AudioContext();
  if (cueCtx.state === "suspended") cueCtx.resume().catch(() => {});
  return cueCtx;
}

function beep(freqs: number[], dur = 0.12, gap = 0.02, type: OscillatorType = "sine") {
  const ctx = getCueCtx();
  let t = ctx.currentTime + 0.01;
  for (const f of freqs) {
    const osc = ctx.createOscillator();
    const g = ctx.createGain();
    osc.type = type;
    osc.frequency.value = f;
    g.gain.setValueAtTime(0.0001, t);
    g.gain.exponentialRampToValueAtTime(0.25, t + 0.012);
    g.gain.exponentialRampToValueAtTime(0.0001, t + dur);
    osc.connect(g);
    g.connect(ctx.destination);
    osc.start(t);
    osc.stop(t + dur + 0.02);
    t += dur + gap;
  }
}

export function beepPreset(kind: string) {
  if (kind === "beep-up") beep([660, 990], 0.1);
  else if (kind === "beep-down") beep([880, 520], 0.12);
  else if (kind === "beep-double") beep([880, 880], 0.07, 0.05);
  else beep([770], 0.12);
}

/** 按设置播放开始/结束提示音。 */
export function playCue(which: "start" | "end") {
  const prefs = useDictPrefs.getState().prefs;
  if (!prefs.cueEnabled) return;
  const kind = which === "start" ? prefs.cueStart : prefs.cueEnd;
  if (!kind || kind === "none") return;
  try {
    if (kind === "custom") {
      const data = localStorage.getItem(
        which === "start" ? "dictCueStartData" : "dictCueEndData",
      );
      if (data) {
        const audio = new Audio(data);
        audio.volume = 0.85;
        audio.play().catch(() => {});
      } else {
        beepPreset(which === "start" ? "beep-up" : "beep-down");
      }
      return;
    }
    beepPreset(kind);
  } catch {
    /* noop */
  }
}
