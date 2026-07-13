// 音频调校的 PCM 与 DSP 已在 Rust；前端只消费波形摘要和临时试听文件。
import { convertFileSrc } from "@tauri-apps/api/core";
import { CMD, cmd } from "@/lib/tauri";
import { useDictPrefs } from "@/store/useDictPrefs";
import { useAudioStore, emptyMeters } from "@/store/useAudioStore";

type Snapshot = { recording: boolean; rawWaveform: [number, number][]; processedWaveform: [number, number][]; stats?: { inLufs: number; outLufs: number; inPeakDb: number; outPeakDb: number; clippedSamples: number } };
let rawCanvas: HTMLCanvasElement | null = null;
let processedCanvas: HTMLCanvasElement | null = null;
let last: Snapshot | null = null;
let timer: ReturnType<typeof setTimeout> | null = null;
let audio: HTMLAudioElement | null = null;

function draw(canvas: HTMLCanvasElement | null, points: [number, number][], color: string) {
  if (!canvas) return; const context = canvas.getContext("2d"); if (!context) return;
  const { width, height } = canvas; context.clearRect(0, 0, width, height); context.strokeStyle = "rgba(255,255,255,.12)"; context.beginPath(); context.moveTo(0, height / 2); context.lineTo(width, height / 2); context.stroke();
  context.strokeStyle = color; context.beginPath(); points.forEach(([min, max], index) => { const x = index * width / Math.max(1, points.length - 1); context.moveTo(x, (1 - (max + 1) / 2) * height); context.lineTo(x, (1 - (min + 1) / 2) * height); }); context.stroke();
}
function apply(snapshot: Snapshot) {
  last = snapshot; draw(rawCanvas, snapshot.rawWaveform, "#8a93b0"); draw(processedCanvas, snapshot.processedWaveform, "#fff");
  const stats = snapshot.stats; useAudioStore.setState({ recording: snapshot.recording, canPlay: !snapshot.recording && snapshot.rawWaveform.length > 0, meters: stats ? { olufs: `${stats.inLufs.toFixed(1)} LUFS`, orms: "-", opeak: stats.inPeakDb.toFixed(1), plufs: `${stats.outLufs.toFixed(1)} LUFS`, prms: "-", ppeak: stats.outPeakDb.toFixed(1), clip: String(stats.clippedSamples) } : { ...emptyMeters } });
}
export function setCanvases(raw: HTMLCanvasElement | null, processed: HTMLCanvasElement | null) { rawCanvas = raw; processedCanvas = processed; if (last) apply(last); }
export async function toggleRecord() { try { if (useAudioStore.getState().recording) { const snapshot = await cmd<Snapshot>(CMD.audioLabStop); apply(snapshot); await reprocess(); useAudioStore.setState({ recInfo: "录音完成", recTone: "ok" }); } else { const snapshot = await cmd<Snapshot>(CMD.audioLabStart, { deviceName: useDictPrefs.getState().prefs.micDeviceId || undefined }); apply(snapshot); useAudioStore.setState({ recInfo: "录音中…", recTone: "" }); } } catch (error) { useAudioStore.setState({ recInfo: `录音失败：${error}`, recTone: "err", recording: false }); } }
export async function reprocess() { const snapshot = await cmd<Snapshot>(CMD.audioLabReprocess, { params: useDictPrefs.getState().dspParams() }); apply(snapshot); }
export function paramChanged() { if (timer) clearTimeout(timer); timer = setTimeout(() => { void reprocess().catch((error) => useAudioStore.setState({ recInfo: `处理失败：${error}`, recTone: "err" })); }, 120); }
async function play(processed: boolean) { try { const path = await cmd<string>(CMD.audioLabAudioPath, { processed }); if (!audio) audio = new Audio(); audio.src = convertFileSrc(path); await audio.play(); } catch (error) { useAudioStore.setState({ recInfo: `播放失败：${error}`, recTone: "err" }); } }
export function playOriginal() { void play(false); }
export function playProcessed() { void play(true); }
export function resetParams() { useAudioStore.setState({ labStatus: "已恢复默认参数并应用到速记。", labStatusTone: "ok" }); paramChanged(); }
