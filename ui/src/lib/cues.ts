import { isWindows } from "@/lib/platform";
import { CMD, cmdSilent } from "@/lib/tauri";

/** 内置音色：每套一对开始/结束音。文件在 `public/cues/<id>.wav`，后端原生播放用同一批 ID。 */
export const CUE_DESIGNS = [
  { name: "经典", start: "beep-up", end: "beep-down" },
  { name: "双响", start: "beep-double", end: "beep-double" },
  { name: "水滴", start: "drop-up", end: "drop-down" },
  { name: "木琴", start: "marimba-up", end: "marimba-down" },
  { name: "低沉木琴", start: "marimba-low-up", end: "marimba-low-down" },
  { name: "玻璃铃", start: "bell-high", end: "bell-low" },
  { name: "轻敲", start: "tick-up", end: "tick-down" },
  { name: "柔和单音", start: "soft-high", end: "soft-low" },
  { name: "三连音", start: "arp-up", end: "arp-down" },
] as const;

type Design = (typeof CUE_DESIGNS)[number];
export type BuiltinCue = Design["start"] | Design["end"];
export type CueKind = "none" | "custom" | BuiltinCue;
export type CueSlot = "start" | "end";

/** 设置里显示的音色名称。 */
export function cueLabel(kind: CueKind): string {
  if (kind === "none") return "无";
  if (kind === "custom") return "自定义文件";
  return CUE_DESIGNS.find((d) => d.start === kind || d.end === kind)?.name ?? kind;
}

/** 试听：与听写时走同一条播放路径（Windows 原生播放，其余回到 WebView）。 */
export function previewCue(kind: CueKind, which: CueSlot) {
  if (kind === "none") return;
  void cmdSilent(CMD.previewDictationCue, { kind, which });
}

let cueCtx: AudioContext | null = null;
const buffers = new Map<string, Promise<AudioBuffer>>();

function getCueCtx(): AudioContext {
  if (!cueCtx || cueCtx.state === "closed") cueCtx = new AudioContext();
  if (cueCtx.state === "suspended") void cueCtx.resume();
  return cueCtx;
}

function loadCue(kind: string): Promise<AudioBuffer> {
  let buffer = buffers.get(kind);
  if (!buffer) {
    buffer = fetch(`/cues/${kind}.wav`)
      .then((res) => {
        if (!res.ok) throw new Error(`HTTP ${res.status}`);
        return res.arrayBuffer();
      })
      // 用离线上下文解码：预加载时不打开系统音频输出。
      .then((data) => new OfflineAudioContext(1, 1, 44100).decodeAudioData(data));
    buffers.set(kind, buffer);
    // 失败的加载不缓存，下次再试。
    buffer.catch(() => buffers.delete(kind));
  }
  return buffer;
}

/**
 * 窗口一挂上监听就把内置音色解码好，第一次播放少等下载和解码。
 * Windows 上内置音色由后端原生播放，WebView 用不到它们。
 */
export function preloadCues() {
  if (isWindows) return;
  for (const design of CUE_DESIGNS) {
    for (const kind of [design.start, design.end]) void loadCue(kind).catch(() => {});
  }
}

/** 后端无法原生播放时（macOS、非 WAV 自定义文件），由收到事件的 WebView 播放。 */
export function playCueKind(kind: string, which: CueSlot) {
  if (!kind || kind === "none") return;
  if (kind === "custom") {
    const data = localStorage.getItem(which === "start" ? "dictCueStartData" : "dictCueEndData");
    if (data) {
      const audio = new Audio(data);
      audio.volume = 0.85;
      void audio.play().catch(() => {});
    }
    return;
  }
  loadCue(kind)
    .then((buffer) => {
      const ctx = getCueCtx();
      const source = ctx.createBufferSource();
      source.buffer = buffer;
      source.connect(ctx.destination);
      source.start();
    })
    .catch((error) => console.error(`[cue] 提示音 ${kind} 播放失败`, error));
}
