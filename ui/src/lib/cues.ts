import { useDictPrefs } from "@/store/useDictPrefs";

let cueCtx: AudioContext | null = null;

function getCueCtx(): AudioContext {
  if (!cueCtx || cueCtx.state === "closed") cueCtx = new AudioContext();
  if (cueCtx.state === "suspended") void cueCtx.resume();
  return cueCtx;
}

function beep(freqs: number[], dur = 0.12, gap = 0.02) {
  const ctx = getCueCtx();
  let time = ctx.currentTime + 0.01;
  for (const freq of freqs) {
    const oscillator = ctx.createOscillator();
    const gain = ctx.createGain();
    oscillator.type = "sine";
    oscillator.frequency.value = freq;
    gain.gain.setValueAtTime(0.0001, time);
    gain.gain.exponentialRampToValueAtTime(0.25, time + 0.012);
    gain.gain.exponentialRampToValueAtTime(0.0001, time + dur);
    oscillator.connect(gain);
    gain.connect(ctx.destination);
    oscillator.start(time);
    oscillator.stop(time + dur + 0.02);
    time += dur + gap;
  }
}

function beepPreset(kind: string) {
  if (kind === "beep-up") beep([660, 990], 0.1);
  else if (kind === "beep-down") beep([880, 520], 0.12);
  else if (kind === "beep-double") beep([880, 880], 0.07, 0.05);
  else beep([770], 0.12);
}

export function playCueKind(kind: string, which: "start" | "end") {
  if (kind === "custom") {
    const data = localStorage.getItem(which === "start" ? "dictCueStartData" : "dictCueEndData");
    if (data) {
      const audio = new Audio(data);
      audio.volume = 0.85;
      void audio.play().catch(() => {});
      return;
    }
  }
  beepPreset(kind);
}

/** 复用迁移前的 Web Audio 提示音，避免原生短流播放产生设备破音。 */
export function playCue(which: "start" | "end") {
  const prefs = useDictPrefs.getState().prefs;
  if (!prefs.cueEnabled) return;
  const kind = which === "start" ? prefs.cueStart : prefs.cueEnd;
  if (!kind || kind === "none") return;
  playCueKind(kind, which);
}
