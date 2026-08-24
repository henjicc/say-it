export type OrbPhase =
  | "idle"
  | "armed"
  | "moving"
  | "positioning"
  | "recording"
  | "processing"
  | "smartProcessing"
  | "success"
  | "fallback"
  | "error"
  | "cancelled"
  | "busy"
  | "submitting"
  | "submitted";

export type OrbClickAction = "activate" | "stop" | "submit";

export const ORB_DRAG_THRESHOLD = 5;
export const FLOATING_ORB_SIZE_RANGE = { min: 44, max: 72 } as const;
export const FLOATING_ORB_OPACITY_RANGE = { min: 40, max: 100 } as const;
export const FLOATING_ORB_GLASS_TINT_RANGE = { min: 0, max: 40 } as const;
export const FLOATING_ORB_GLASS_BORDER_RANGE = { min: 0, max: 30 } as const;
export const FLOATING_ORB_GLASS_MATERIALS = ["underWindow", "content", "sidebar"] as const;
export type FloatingOrbGlassMaterial = (typeof FLOATING_ORB_GLASS_MATERIALS)[number];
export const DEFAULT_FLOATING_ORB_APPEARANCE = {
  size: 48,
  opacity: 40,
  glassEnabled: false,
  glassMaterial: "sidebar" as FloatingOrbGlassMaterial,
  glassTint: 8,
  glassBorder: 0,
} as const;

export interface FloatingOrbAppearance {
  size: number;
  opacity: number;
  glassEnabled: boolean;
  glassMaterial: FloatingOrbGlassMaterial;
  glassTint: number;
  glassBorder: number;
}

function clampInteger(value: unknown, min: number, max: number, fallback: number): number {
  const number = Number(value);
  return Number.isFinite(number) ? Math.round(Math.max(min, Math.min(max, number))) : fallback;
}

export function normalizeFloatingOrbAppearance(payload: {
  size?: number;
  opacity?: number;
  glassEnabled?: boolean;
  glassMaterial?: string;
  glassTint?: number;
  glassBorder?: number;
}): FloatingOrbAppearance {
  return {
    size: clampInteger(
      payload.size,
      FLOATING_ORB_SIZE_RANGE.min,
      FLOATING_ORB_SIZE_RANGE.max,
      DEFAULT_FLOATING_ORB_APPEARANCE.size,
    ),
    opacity: clampInteger(
      payload.opacity,
      FLOATING_ORB_OPACITY_RANGE.min,
      FLOATING_ORB_OPACITY_RANGE.max,
      DEFAULT_FLOATING_ORB_APPEARANCE.opacity,
    ),
    glassEnabled: payload.glassEnabled === true,
    glassMaterial: FLOATING_ORB_GLASS_MATERIALS.includes(
      payload.glassMaterial as FloatingOrbGlassMaterial,
    )
      ? payload.glassMaterial as FloatingOrbGlassMaterial
      : DEFAULT_FLOATING_ORB_APPEARANCE.glassMaterial,
    glassTint: clampInteger(
      payload.glassTint,
      FLOATING_ORB_GLASS_TINT_RANGE.min,
      FLOATING_ORB_GLASS_TINT_RANGE.max,
      DEFAULT_FLOATING_ORB_APPEARANCE.glassTint,
    ),
    glassBorder: clampInteger(
      payload.glassBorder,
      FLOATING_ORB_GLASS_BORDER_RANGE.min,
      FLOATING_ORB_GLASS_BORDER_RANGE.max,
      DEFAULT_FLOATING_ORB_APPEARANCE.glassBorder,
    ),
  };
}

export function shouldStartOrbDrag(deltaX: number, deltaY: number): boolean {
  return Math.hypot(deltaX, deltaY) >= ORB_DRAG_THRESHOLD;
}

export function floatingOrbWaveScale(value: number): number {
  const normalized = Math.max(0, Math.min(1, Number(value) || 0));
  return Math.min(1, Math.sqrt(normalized) * 1.8);
}

export function floatingOrbClickAction(
  phase: OrbPhase,
  canSubmit: boolean,
): OrbClickAction | null {
  if (phase === "armed") return "activate";
  if (phase === "recording") return "stop";
  if (phase === "success" && canSubmit) return "submit";
  return null;
}

export function floatingOrbLabel(
  phase: Exclude<OrbPhase, "idle" | "busy">,
  message: string,
): string {
  if (message) return message;
  return ({
    moving: "正在启动…",
    positioning: "正在定位…",
    armed: "点击开始语音输入",
    recording: "聆听中…",
    processing: "识别中…",
    smartProcessing: "处理中…",
    success: "已完成并复制",
    fallback: "已复制，请手动粘贴",
    error: "语音输入失败",
    cancelled: "已取消",
    submitting: "正在发送回车…",
    submitted: "已发送回车",
  } as const)[phase];
}
