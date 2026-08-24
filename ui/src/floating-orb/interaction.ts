export type OrbPhase =
  | "idle"
  | "moving"
  | "recording"
  | "processing"
  | "smartProcessing"
  | "success"
  | "fallback"
  | "error"
  | "busy";

export const ORB_DRAG_THRESHOLD = 5;

export function shouldStartOrbDrag(deltaX: number, deltaY: number): boolean {
  return Math.hypot(deltaX, deltaY) >= ORB_DRAG_THRESHOLD;
}

export function floatingOrbLabel(
  phase: Exclude<OrbPhase, "idle" | "busy">,
  message: string,
  stopHovered: boolean,
): string {
  if (stopHovered && phase === "recording") return "停止识别";
  if (message) return message;
  return ({
    moving: "正在启动…",
    recording: "聆听中…",
    processing: "识别中…",
    smartProcessing: "处理中…",
    success: "已完成并复制",
    fallback: "已复制，请手动粘贴",
    error: "语音输入失败",
  } as const)[phase];
}
